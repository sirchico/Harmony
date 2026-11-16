//! API v2 host requests the extension worker cannot answer from local files: Discord lookups
//! and the media of the message an action was chosen on. Everything here is bounded and owned
//! by one invocation; nothing is cached for extensions.
use client_core::{Command, State};
use extensions::{
	AttachmentSource, HostRequest, HostResponse, Invocation, MediaKind, MediaLink, MediaSource,
	RequestErrorCode, ResponseValue,
};
use model::Id;
use std::time::{Duration, Instant};
use ui::ExtensionContext;

/// Most invocations waiting on Discord at once; later ones are answered `limit`.
pub(crate) const MAX_LOOKUPS: usize = 4;
/// A lookup Discord never answers fails instead of holding the invocation forever.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(20);
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

pub(crate) struct Lookup {
	pub id: String,
	pub account: String,
	pub generation: u64,
	pub invocation: Box<Invocation>,
	pub context: ExtensionContext,
	/// (client-core request number, the extension's request ID).
	waiting: Vec<(u64, String)>,
	deadline: Instant,
}

#[derive(Default)]
pub(crate) struct Lookups {
	next_request: u64,
	active: Vec<Lookup>,
	commands: Vec<Command>,
}

impl Lookups {
	pub fn clear(&mut self) {
		self.active.clear();
		self.commands.clear();
	}
	/// Queues a command for the app to send, such as a message released by its hooks.
	pub fn push_command(&mut self, command: Command) {
		self.commands.push(command);
	}
	pub fn take_commands(&mut self) -> Vec<Command> {
		std::mem::take(&mut self.commands)
	}
	#[cfg(test)]
	pub fn is_empty(&self) -> bool {
		self.active.is_empty()
	}

	/// Starts the remote part of one request round. Returns the invocation at once when nothing
	/// needs to wait for Discord (every request was answered with an error).
	#[allow(clippy::too_many_arguments)]
	pub fn start(
		&mut self,
		state: &State,
		id: String,
		account: String,
		generation: u64,
		mut invocation: Box<Invocation>,
		context: ExtensionContext,
		requests: Vec<HostRequest>,
	) -> Option<Lookup> {
		let busy = self.active.len() >= MAX_LOOKUPS;
		let mut waiting = Vec::new();
		for request in requests {
			let answer = match &request {
				_ if busy => Some(HostResponse::error(
					request.id(),
					RequestErrorCode::Limit,
					"Too many lookups are already running",
				)),
				HostRequest::ServerProfile { id, guild_id } => {
					match guild_id
						.parse::<u64>()
						.ok()
						.filter(|guild| state.guilds.iter().any(|known| known.id == Id(*guild)))
					{
						Some(guild) => {
							self.next_request = self.next_request.wrapping_add(1);
							let number = self.next_request;
							self.commands.push(Command::ServerProfile {
								guild: Id(guild),
								request: number,
							});
							waiting.push((number, id.clone()));
							None
						}
						None => Some(HostResponse::error(
							id,
							RequestErrorCode::NotFound,
							"Only servers this account has joined can be looked up",
						)),
					}
				}
				other => Some(HostResponse::error(
					other.id(),
					RequestErrorCode::Unsupported,
					"This request is not available here",
				)),
			};
			invocation.responses.extend(answer);
		}
		let lookup = Lookup {
			id,
			account,
			generation,
			invocation,
			context,
			waiting,
			deadline: Instant::now() + LOOKUP_TIMEOUT,
		};
		if lookup.waiting.is_empty() {
			return Some(lookup);
		}
		self.active.push(lookup);
		None
	}

	/// Collects finished Discord lookups. Returns invocations ready to continue.
	pub fn poll(&mut self, state: &mut State, now: Instant) -> Vec<Lookup> {
		while let Some((request, result)) = state.server_profiles.pop_front() {
			let Some(lookup) = self
				.active
				.iter_mut()
				.find(|lookup| lookup.waiting.iter().any(|(number, _)| *number == request))
			else {
				continue;
			};
			let index = lookup
				.waiting
				.iter()
				.position(|(number, _)| *number == request)
				.unwrap();
			let (_, id) = lookup.waiting.swap_remove(index);
			let response = match result {
				Ok(profile) => {
					let profile = public_profile(&profile);
					if profile.validate().is_ok() {
						HostResponse::ok(
							&id,
							ResponseValue::ServerProfile {
								profile: Box::new(profile),
							},
						)
					} else {
						HostResponse::error(
							&id,
							RequestErrorCode::Failed,
							"Server details were invalid",
						)
					}
				}
				Err(client_core::auth::Failure::Capacity) => HostResponse::error(
					&id,
					RequestErrorCode::Limit,
					"Harmony is busy; try again shortly",
				),
				Err(_) => HostResponse::error(
					&id,
					RequestErrorCode::Failed,
					"Discord did not return this server",
				),
			};
			lookup.invocation.responses.push(response);
		}
		for lookup in &mut self.active {
			if now >= lookup.deadline {
				for (_, id) in lookup.waiting.drain(..) {
					lookup.invocation.responses.push(HostResponse::error(
						&id,
						RequestErrorCode::Unavailable,
						"Discord did not answer in time",
					));
				}
			}
		}
		let (ready, waiting) = std::mem::take(&mut self.active)
			.into_iter()
			.partition(|lookup| lookup.waiting.is_empty());
		self.active = waiting;
		ready
	}
}

fn cdn(kind: &str, guild: Id, hash: &Option<String>) -> Option<String> {
	hash.as_ref().map(|hash| {
		let extension = if hash.starts_with("a_") { "gif" } else { "png" };
		format!("https://cdn.discordapp.com/{kind}/{guild}/{hash}.{extension}?size=1024")
	})
}

pub(crate) fn public_profile(
	profile: &model::server_profile::ServerProfile,
) -> extensions::ServerProfile {
	extensions::ServerProfile {
		id: profile.id.to_string(),
		name: profile.name.clone(),
		description: profile.description.clone(),
		icon_url: cdn("icons", profile.id, &profile.icon),
		banner_url: cdn("banners", profile.id, &profile.banner),
		splash_url: cdn("splashes", profile.id, &profile.splash),
		owner_id: profile.owner_id.map(|id| id.to_string()),
		created_at_ms: (profile.id.0 >> 22) + DISCORD_EPOCH_MS,
		verification_level: profile.verification_level,
		explicit_content_filter: profile.explicit_content_filter,
		nsfw_level: profile.nsfw_level,
		mfa_level: profile.mfa_level,
		premium_tier: profile.premium_tier,
		premium_subscription_count: profile.premium_subscription_count,
		approximate_member_count: profile.approximate_member_count,
		approximate_presence_count: profile.approximate_presence_count,
		max_members: profile.max_members,
		vanity_url_code: profile.vanity_url_code.clone(),
		preferred_locale: profile.preferred_locale.clone(),
		features: profile.features.clone(),
		roles: profile.roles,
		emojis: profile.emojis,
		stickers: profile.stickers,
	}
}

fn https(url: &Option<String>) -> Option<&str> {
	url.as_deref().filter(|url| url.starts_with("https://"))
}

/// Fills the public media links and readable attachments of the message an action was chosen
/// on, only for the capabilities the extension holds.
pub(crate) fn attach_message_media(
	invocation: &mut Invocation,
	state: &State,
	media_links: bool,
	attachment_contents: bool,
) {
	let Some(message) = invocation
		.selected_message_ref
		.as_ref()
		.and_then(|target| target.message_id.parse::<u64>().ok())
		.and_then(|id| state.timeline.get(Id(id)))
	else {
		return;
	};
	if attachment_contents {
		invocation.attachment_sources = message
			.attachments
			.iter()
			.filter_map(|attachment| {
				Some(AttachmentSource {
					id: attachment.id.to_string(),
					filename: attachment.filename.clone(),
					content_type: attachment.content_type.clone(),
					size: attachment.size,
					url: https(&attachment.media.url)?.to_owned(),
				})
			})
			.collect();
	}
	if !media_links {
		return;
	}
	let mut links = Vec::new();
	for attachment in &message.attachments {
		let Some(url) = https(&attachment.media.url) else {
			continue;
		};
		let kind = attachment.content_type.as_deref().unwrap_or("");
		links.push(MediaLink {
			source: MediaSource::Attachment,
			kind: if kind.starts_with("image/") {
				MediaKind::Image
			} else if kind.starts_with("video/") {
				MediaKind::Video
			} else if kind.starts_with("audio/") {
				MediaKind::Audio
			} else {
				MediaKind::File
			},
			url: url.to_owned(),
			attachment_id: Some(attachment.id.to_string()),
			filename: Some(attachment.filename.clone()),
			content_type: attachment.content_type.clone(),
			size: Some(attachment.size),
			width: (attachment.media.width > 0).then_some(attachment.media.width),
			height: (attachment.media.height > 0).then_some(attachment.media.height),
		});
	}
	for embed in &message.embeds {
		for (media, kind) in [
			(&embed.image, MediaKind::Image),
			(&embed.thumbnail, MediaKind::Image),
			(&embed.video, MediaKind::Video),
		] {
			let Some(media) = media else { continue };
			let Some(url) = https(&media.url) else {
				continue;
			};
			if links.iter().any(|link: &MediaLink| link.url == url) {
				continue;
			}
			links.push(MediaLink {
				source: MediaSource::Embed,
				kind,
				url: url.to_owned(),
				attachment_id: None,
				filename: None,
				content_type: None,
				size: None,
				width: (media.width > 0).then_some(media.width),
				height: (media.height > 0).then_some(media.height),
			});
		}
	}
	links.truncate(extensions::MAX_MEDIA_LINKS);
	invocation.media = Some(links);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn unknown_servers_and_unsupported_requests_answer_without_waiting() {
		let mut lookups = Lookups::default();
		let state = State::default();
		let ready = lookups
			.start(
				&state,
				"server-info".into(),
				"account".into(),
				0,
				Box::default(),
				ExtensionContext::panel(&state),
				vec![
					HostRequest::ServerProfile {
						id: "a".into(),
						guild_id: "123".into(),
					},
					HostRequest::KvGet {
						id: "b".into(),
						keys: vec!["k".into()],
					},
				],
			)
			.expect("nothing to wait for");
		let codes: Vec<_> = ready
			.invocation
			.responses
			.iter()
			.map(|response| response.error.as_ref().unwrap().code)
			.collect();
		assert_eq!(
			codes,
			[RequestErrorCode::NotFound, RequestErrorCode::Unsupported]
		);
		assert!(lookups.take_commands().is_empty() && lookups.is_empty());
	}

	#[test]
	fn joined_server_lookup_waits_for_its_own_result_then_times_out_others() {
		let mut lookups = Lookups::default();
		let mut state = State::default();
		state.guilds.push(model::Guild {
			default_message_notifications: None,
			stickers: None,
			emojis: None,
			id: Id(10),
			name: "Synthetic".into(),
			icon: None,
		});
		let request = |id: &str| HostRequest::ServerProfile {
			id: id.into(),
			guild_id: "10".into(),
		};
		for name in ["first", "second"] {
			assert!(
				lookups
					.start(
						&state,
						name.into(),
						"account".into(),
						0,
						Box::default(),
						ExtensionContext::panel(&state),
						vec![request(name)],
					)
					.is_none()
			);
		}
		let commands = lookups.take_commands();
		let Some(Command::ServerProfile {
			request: second, ..
		}) = commands.get(1)
		else {
			panic!("expected two lookups");
		};
		// A stale result for nobody, then the second lookup's own answer.
		state.finish_server_profile(999, Err(client_core::auth::Failure::Capacity));
		state.finish_server_profile(
			*second,
			Ok(Box::new(model::server_profile::ServerProfile {
				id: Id(10),
				name: "Synthetic".into(),
				description: None,
				icon: Some("a_abc".into()),
				banner: None,
				splash: None,
				owner_id: Some(Id(20)),
				verification_level: None,
				explicit_content_filter: None,
				nsfw_level: None,
				mfa_level: None,
				premium_tier: None,
				premium_subscription_count: None,
				approximate_member_count: Some(3),
				approximate_presence_count: None,
				max_members: None,
				vanity_url_code: None,
				preferred_locale: None,
				features: Vec::new(),
				roles: 1,
				emojis: 0,
				stickers: 0,
			})),
		);
		let now = Instant::now();
		let ready = lookups.poll(&mut state, now);
		assert_eq!(ready.len(), 1);
		assert_eq!(ready[0].id, "second");
		let Some(ResponseValue::ServerProfile { profile }) =
			&ready[0].invocation.responses[0].value
		else {
			panic!("expected a profile");
		};
		assert_eq!(
			profile.icon_url.as_deref(),
			Some("https://cdn.discordapp.com/icons/10/a_abc.gif?size=1024")
		);
		let late = lookups.poll(&mut state, now + LOOKUP_TIMEOUT);
		assert_eq!(late[0].id, "first");
		assert_eq!(
			late[0].invocation.responses[0].error.as_ref().unwrap().code,
			RequestErrorCode::Unavailable
		);
		assert!(lookups.is_empty());
	}
}
