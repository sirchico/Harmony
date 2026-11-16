//! Admit bounded live events only after the state reducer accepts their timeline changes.
use client_core::{Event, State, auth::AuthState};
use extensions::{MAX_EVENT_CONTENT_BYTES, MessageEvent, MessageEventKind};
use model::{Freshness, Id, Message, Patch};

pub fn available(state: &State) -> bool {
	state.user.as_ref().is_some_and(|user| user.id.0 != 0)
		&& (state.demo || state.auth == AuthState::Authenticated)
		&& state.gateway_connected
		&& state.freshness != Freshness::Unavailable
		&& state.selected.is_some_and(|channel| {
			state.can_read_history(channel)
				&& state
					.channel(channel)
					.is_some_and(|channel| channel.supports_text())
		})
}

/// Names and placement for a message, as Harmony shows them.
fn describe(state: &State, message: &Message) -> (String, Option<String>, Option<String>, bool) {
	let channel = state.channel(message.channel);
	let guild = channel.and_then(|channel| channel.guild);
	let place = channel.map(|channel| {
		match guild.and_then(|guild| state.guilds.iter().find(|known| known.id == guild)) {
			Some(server) => format!("{} · #{}", server.name, channel.name),
			None => channel.name.clone(),
		}
	});
	let own = state.user.as_ref().map(|user| user.id);
	let mentions_me = message.mention_everyone
		|| own.is_some_and(|own| message.mentions.iter().any(|user| user.id == own));
	(
		state.message_author_name(message).to_owned(),
		guild.map(|guild| guild.0.to_string()),
		place
			.filter(|place| !place.is_empty())
			.map(|place| place.chars().take(200).collect()),
		mentions_me,
	)
}

fn public(message: &Message) -> bool {
	!message.ephemeral && message.flags & 64 == 0
}

pub struct Candidate {
	generation: u64,
	state_revision: u64,
	channel: Id,
	id: Id,
	revision: Option<u64>,
	kind: MessageEventKind,
	content: bool,
}

pub fn capture(state: &State, event: &Event) -> Vec<Candidate> {
	if !available(state) {
		return Vec::new();
	}
	let (kind, channel, ids, content) = match event {
		Event::Message(message)
			if public(message)
				&& message.author.id.0 != 0
				&& message.content.len() <= MAX_EVENT_CONTENT_BYTES =>
		{
			(
				MessageEventKind::Create,
				message.channel,
				std::slice::from_ref(&message.id),
				true,
			)
		}
		Event::Patch(patch)
			if !matches!(patch.flags, Patch::Value(flags) if flags & 64 != 0)
				&& !matches!(&patch.content, Patch::Value(text) if text.len() > MAX_EVENT_CONTENT_BYTES) =>
		{
			(
				MessageEventKind::Update,
				patch.channel,
				std::slice::from_ref(&patch.id),
				!matches!(patch.content, Patch::Absent),
			)
		}
		Event::Delete { channel, id } => (
			MessageEventKind::Delete,
			*channel,
			std::slice::from_ref(id),
			false,
		),
		Event::DeleteBulk { channel, ids } if ids.len() <= 100 => {
			(MessageEventKind::Delete, *channel, ids.as_slice(), false)
		}
		_ => return Vec::new(),
	};
	if state.selected != Some(channel) || channel.0 == 0 {
		return Vec::new();
	}
	// Fixed-size descriptors contain no message bodies; at most 100 fit one bulk delete.
	let mut candidates: Vec<Candidate> = Vec::with_capacity(ids.len());
	for &id in ids {
		let previous = state
			.timeline
			.get(id)
			.filter(|message| message.channel == channel && public(message));
		if id.0 == 0
			|| (kind != MessageEventKind::Create && previous.is_none())
			|| candidates.iter().any(|candidate| candidate.id == id)
		{
			continue;
		}
		candidates.push(Candidate {
			generation: state.generation,
			state_revision: state.revision,
			channel,
			id,
			revision: previous.map(|message| message.revision),
			kind,
			content,
		});
	}
	candidates
}

/// A new message in a conversation other than the open one, for extensions granted
/// `all_message_events`. Built from the incoming message itself: other conversations are not
/// in the loaded timeline. Only public messages from other people in readable channels.
pub fn capture_elsewhere(state: &State, event: &Event) -> Option<MessageEvent> {
	let Event::Message(message) = event else {
		return None;
	};
	let own = state.user.as_ref().map(|user| user.id);
	(state.user.is_some()
		&& (state.demo || state.auth == AuthState::Authenticated)
		&& state.gateway_connected
		&& state.selected != Some(message.channel)
		&& message.channel.0 != 0
		&& public(message)
		&& message.author.id.0 != 0
		&& Some(message.author.id) != own
		&& !message.content.is_empty()
		&& message.content.len() <= MAX_EVENT_CONTENT_BYTES
		&& state.can_read_history(message.channel))
	.then(|| {
		let (author_name, guild_id, place, mentions_me) = describe(state, message);
		MessageEvent {
			kind: MessageEventKind::Create,
			channel_id: message.channel.0.to_string(),
			message_id: message.id.0.to_string(),
			author_id: Some(message.author.id.0.to_string()),
			content: Some(message.content.clone()),
			author_name: Some(author_name.chars().take(200).collect()),
			guild_id,
			place,
			mentions_me,
			open: false,
		}
	})
}

impl Candidate {
	pub fn admit(self, state: &State) -> Option<MessageEvent> {
		if !available(state)
			|| self.generation != state.generation
			|| self.state_revision == state.revision
			|| state.selected != Some(self.channel)
		{
			return None;
		}
		let mut described = (None, None, None, false);
		let (author_id, content) = if self.kind == MessageEventKind::Delete {
			if !state.timeline.is_deleted(self.id) {
				return None;
			}
			(None, None)
		} else {
			let message = state.timeline.get(self.id)?;
			if message.channel != self.channel
				|| !public(message)
				|| message.author.id.0 == 0
				|| self.revision == Some(message.revision)
				|| (self.content && message.content.len() > MAX_EVENT_CONTENT_BYTES)
			{
				return None;
			}
			let (author_name, guild_id, place, mentions_me) = describe(state, message);
			described = (
				Some(author_name.chars().take(200).collect()),
				guild_id,
				place,
				mentions_me,
			);
			(
				Some(message.author.id.0.to_string()),
				self.content.then(|| message.content.clone()),
			)
		};
		let (author_name, guild_id, place, mentions_me) = described;
		Some(MessageEvent {
			kind: self.kind,
			channel_id: self.channel.0.to_string(),
			message_id: self.id.0.to_string(),
			author_id,
			content,
			author_name,
			guild_id,
			place,
			mentions_me,
			open: true,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use client_core::Envelope;
	use model::MessagePatch;

	fn apply(state: &mut State, event: Event) -> Vec<MessageEvent> {
		let candidates = capture(state, &event);
		state.apply(Envelope {
			generation: state.generation,
			event,
		});
		candidates
			.into_iter()
			.filter_map(|candidate| candidate.admit(state))
			.collect()
	}

	fn patch(id: u64, content: Patch<String>, edited: Patch<i128>) -> MessagePatch {
		MessagePatch {
			poll: model::Patch::Absent,
			id: Id(id),
			channel: Id(20),
			content,
			edited,
			sticker_items: Patch::Absent,
			flags: Patch::Absent,
			components: Patch::Absent,
			application_id: Patch::Absent,
			extra_content: Default::default(),
			reactions: Patch::Absent,
			mentions: Patch::Absent,
			embeds: Patch::Absent,
			embeds_suppressed: Patch::Absent,
			attachments: Patch::Absent,
		}
	}

	#[test]
	fn only_active_public_accepted_gateway_messages_are_admitted() {
		let mut state = test_support::demo_state();
		let message = test_support::message(1001, Id(20));
		let events = apply(&mut state, Event::Message(message.clone()));
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].kind, MessageEventKind::Create);
		assert_eq!(events[0].content.as_deref(), Some(message.content.as_str()));
		assert!(apply(&mut state, Event::Message(message.clone())).is_empty());
		assert!(
			apply(
				&mut state,
				Event::Message(test_support::message(1002, Id(21)))
			)
			.is_empty()
		);
		for kind in 0..4 {
			let mut invalid = test_support::message(1010 + kind, Id(20));
			match kind {
				0 => invalid.ephemeral = true,
				1 => invalid.flags = 64,
				2 => invalid.content = "x".repeat(MAX_EVENT_CONTENT_BYTES + 1),
				_ => invalid.reply_deleted = true,
			}
			assert!(apply(&mut state, Event::Message(invalid)).is_empty());
		}
		assert!(
			capture(
				&state,
				&Event::History {
					channel: Id(20),
					request: state.request,
					older: false,
					messages: vec![message.clone()]
				}
			)
			.is_empty()
		);
		assert!(
			capture(
				&state,
				&Event::SendResult {
					nonce: "synthetic".into(),
					result: Ok(message)
				}
			)
			.is_empty()
		);
		let event = Event::Message(test_support::message(1030, Id(20)));
		let candidates = capture(&state, &event);
		state.apply(Envelope {
			generation: state.generation.wrapping_sub(1),
			event,
		});
		assert!(
			candidates
				.into_iter()
				.all(|candidate| candidate.admit(&state).is_none())
		);
		apply(&mut state, Event::Disconnected);
		assert!(!available(&state));
	}

	#[test]
	fn accepted_patches_emit_current_content_while_stale_or_private_patches_do_not() {
		let mut state = test_support::demo_state();
		let events = apply(
			&mut state,
			Event::Patch(patch(
				499,
				Patch::Value("updated".into()),
				Patch::Value(100),
			)),
		);
		assert_eq!(events[0].kind, MessageEventKind::Update);
		assert_eq!(events[0].content.as_deref(), Some("updated"));
		assert!(
			apply(
				&mut state,
				Event::Patch(patch(499, Patch::Value("stale".into()), Patch::Value(99)))
			)
			.is_empty()
		);
		let events = apply(
			&mut state,
			Event::Patch(patch(499, Patch::Absent, Patch::Value(101))),
		);
		assert!(events[0].content.is_none());
		let mut private = patch(499, Patch::Value("private".into()), Patch::Value(102));
		private.flags = Patch::Value(64);
		assert!(apply(&mut state, Event::Patch(private)).is_empty());
		assert!(
			apply(
				&mut state,
				Event::Patch(patch(9999, Patch::Value("unknown".into()), Patch::Absent))
			)
			.is_empty()
		);
	}

	#[test]
	fn deletions_only_reveal_loaded_public_ids_and_bulk_is_bounded() {
		let mut state = test_support::demo_state();
		state.set_preserve_deleted_messages(true);
		let events = apply(
			&mut state,
			Event::DeleteBulk {
				channel: Id(20),
				ids: vec![Id(498), Id(499), Id(499), Id(9999)],
			},
		);
		assert_eq!(events.len(), 2);
		assert!(
			events
				.iter()
				.all(|event| event.kind == MessageEventKind::Delete
					&& event.author_id.is_none()
					&& event.content.is_none())
		);
		assert!(
			apply(
				&mut state,
				Event::Delete {
					channel: Id(20),
					id: Id(499)
				}
			)
			.is_empty()
		);
		assert!(
			capture(
				&state,
				&Event::DeleteBulk {
					channel: Id(20),
					ids: vec![Id(500); 101]
				}
			)
			.is_empty()
		);
		let event = Event::Delete {
			channel: Id(20),
			id: Id(500),
		};
		let candidates = capture(&state, &event);
		state.freshness = Freshness::Unavailable;
		state.apply(Envelope {
			generation: state.generation,
			event,
		});
		assert!(
			candidates
				.into_iter()
				.all(|candidate| candidate.admit(&state).is_none())
		);
		state = test_support::demo_state();
		state.demo = false;
		state.auth = AuthState::Unauthenticated;
		assert!(!available(&state));
	}
}
