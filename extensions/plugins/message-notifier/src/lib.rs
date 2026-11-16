//! Message Notifier: a system notification when chosen people send a message in a
//! conversation you are not looking at. Choose people from their menu or by ID in settings.
//! Written for Harmony using only the public API v2 SDK.
use harmony_extension_sdk::{
	Element, ExtensionInvocation, ExtensionOutput, MenuTarget, MessageEventKind, Notification,
};
use std::collections::BTreeSet;

const MAX_WATCHED: usize = 100;

/// People chosen from their menu, saved as a JSON list of IDs.
fn saved(input: &ExtensionInvocation) -> BTreeSet<String> {
	input
		.invocation
		.invocation
		.invocation
		.storage_json::<BTreeSet<String>>()
		.ok()
		.flatten()
		.unwrap_or_default()
}

/// IDs typed in settings: digits separated by commas or spaces.
pub fn typed(text: &str) -> BTreeSet<String> {
	text.split(|c: char| c == ',' || c.is_whitespace())
		.map(str::trim)
		.filter(|id| !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()))
		.map(str::to_owned)
		.collect()
}

fn on_message(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut output = ExtensionOutput::default();
	let Some(event) = &input.message else {
		return output;
	};
	if event.kind != MessageEventKind::Create || event.open {
		return output;
	}
	let Some(author) = &event.author_id else {
		return output;
	};
	let watched = saved(input).contains(author)
		|| typed(input.setting_text("users").unwrap_or_default()).contains(author);
	if !watched {
		return output;
	}
	let name = event
		.author_name
		.clone()
		.unwrap_or_else(|| "Someone".into());
	let place = event.place.clone().unwrap_or_else(|| "their DMs".into());
	let body = if input.setting_bool("preview") {
		let text: String = event
			.content
			.as_deref()
			.unwrap_or_default()
			.chars()
			.take(200)
			.collect();
		format!("{place}: {text}")
	} else {
		format!("Click to jump to {place}")
	};
	output.notifications.push(Notification {
		title: format!("{name} sent a message"),
		body,
		channel_id: Some(event.channel_id.clone()),
		message_id: Some(event.message_id.clone()),
	});
	output
}

fn toggle(input: &ExtensionInvocation) -> ExtensionOutput {
	let Some(MenuTarget::User { user_id, .. }) = &input.menu_target else {
		return ExtensionOutput::panel(vec![Element::Text {
			text: "Open this from a person's menu.".into(),
		}]);
	};
	let mut watched = saved(input);
	let text = if watched.remove(user_id) {
		"You will no longer be notified about their messages."
	} else if watched.len() >= MAX_WATCHED {
		"You already follow 100 people; remove someone first."
	} else {
		watched.insert(user_id.clone());
		"You will be notified when they send a message somewhere you are not looking."
	};
	let mut output = ExtensionOutput::panel(vec![
		Element::Heading {
			text: "Message Notifier".into(),
		},
		Element::Text { text: text.into() },
	]);
	let _ = output.output.output.set_storage_json(&watched);
	output
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	match input.action() {
		"toggle" => toggle(&input),
		_ => on_message(&input),
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{MessageEventInfo, SettingValue, testing::TestHost};

	fn event(author: &str, open: bool) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "message".into();
		input.message = Some(MessageEventInfo {
			kind: MessageEventKind::Create,
			channel_id: "30".into(),
			message_id: "31".into(),
			author_id: Some(author.into()),
			content: Some("hello".into()),
			author_name: Some("Robin".into()),
			place: Some("Game Night · #general".into()),
			open,
			..Default::default()
		});
		input
	}

	#[test]
	fn watched_people_notify_only_outside_the_open_conversation() {
		let mut chosen = event("42", false);
		chosen.invocation.invocation.invocation.storage = Some(r#"["42"]"#.into());
		let mut host = TestHost::default();
		host.run(chosen.clone(), handle).unwrap();
		assert_eq!(
			host.notifications,
			[Notification {
				title: "Robin sent a message".into(),
				body: "Click to jump to Game Night · #general".into(),
				channel_id: Some("30".into()),
				message_id: Some("31".into()),
			}]
		);
		let mut open = chosen.clone();
		open.message.as_mut().unwrap().open = true;
		let mut stranger = event("7", false);
		stranger
			.settings
			.insert("users".into(), SettingValue::Text("8, 9".into()));
		let mut quiet = TestHost::default();
		quiet.run(open, handle).unwrap();
		quiet.run(stranger, handle).unwrap();
		assert!(quiet.notifications.is_empty());
		let mut typed_in = event("9", false);
		typed_in
			.settings
			.insert("users".into(), SettingValue::Text("8, 9".into()));
		let mut host = TestHost::default();
		host.run(typed_in, handle).unwrap();
		assert_eq!(host.notifications.len(), 1);
	}

	#[test]
	fn the_menu_entry_toggles_a_person() {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "toggle".into();
		input.menu_target = Some(MenuTarget::User {
			user_id: "42".into(),
			guild_id: None,
			channel_id: None,
		});
		let added = TestHost::default().run(input.clone(), handle).unwrap();
		assert_eq!(added.output.output.storage.as_deref(), Some(r#"["42"]"#));
		input.invocation.invocation.invocation.storage = added.output.output.storage;
		let removed = TestHost::default().run(input, handle).unwrap();
		assert_eq!(removed.output.output.storage.as_deref(), Some("[]"));
	}
}
