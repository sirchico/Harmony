//! User Tags: put your own short label on people. Harmony draws it after their name in chat
//! and the member list (`user.badge`), and adds a composer button (`composer.button`) that
//! lists every tag. Written for Harmony using only the public API v2 SDK; tags stay in this
//! extension's local storage and are redrawn by the activation action when Harmony loads.
use harmony_extension_sdk::{
	ContributionIcon, ContributionItem, ContributionOp, ContributionTarget, Element,
	ExtensionInvocation, ExtensionOutput, MenuTarget, Slot,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_TAGS: usize = 200;
const MAX_TAG_CHARS: usize = 24;
/// (label shown in the color picker, badge color).
pub const COLORS: &[(&str, &str)] = &[
	("Blurple", "#5865F2"),
	("Green", "#3BA55C"),
	("Yellow", "#FAA61A"),
	("Red", "#ED4245"),
	("Pink", "#EB459E"),
	("Gray", "#80848E"),
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
	pub text: String,
	pub color: String,
}

type Tags = BTreeMap<String, Tag>;

fn load(input: &ExtensionInvocation) -> Tags {
	input
		.invocation
		.invocation
		.invocation
		.storage_json::<Tags>()
		.ok()
		.flatten()
		.unwrap_or_default()
}

fn badge(user: &str, tag: &Tag) -> ContributionOp {
	ContributionOp::set(
		Slot::UserBadge,
		format!("u{user}"),
		ContributionTarget::user(user),
		ContributionItem::text(&tag.text)
			.color(&tag.color)
			.icon(ContributionIcon::Tag)
			.tooltip("Your tag (User Tags)"),
	)
}

fn composer_button() -> ContributionOp {
	ContributionOp::set(
		Slot::ComposerButton,
		"list",
		ContributionTarget::App,
		ContributionItem::text("User tags")
			.tooltip("Show your user tags")
			.icon(ContributionIcon::Tag)
			.action("list"),
	)
}

fn save(output: &mut ExtensionOutput, tags: &Tags) {
	if let Err(error) = output.output.output.set_storage_json(tags) {
		output.output.output.panel.push(Element::Text {
			text: format!("Could not save tags: {error:?}"),
		});
	}
}

/// The trimmed tag text, or why it cannot be used.
pub fn clean(text: &str) -> Result<String, &'static str> {
	let text = text.trim();
	if text.is_empty() {
		return Err("Type a tag first.");
	}
	if text.chars().count() > MAX_TAG_CHARS || text.chars().any(char::is_control) {
		return Err("Tags are one line of at most 24 characters.");
	}
	Ok(text.to_owned())
}

fn editor(user: &str, tag: Option<&Tag>, note: Option<&str>) -> Vec<Element> {
	let mut panel = vec![Element::Heading {
		text: "Tag this person".into(),
	}];
	if let Some(note) = note {
		panel.push(Element::Text { text: note.into() });
	}
	panel.extend([
		Element::TextInput {
			id: "text".into(),
			label: "Tag".into(),
			value: tag.map(|tag| tag.text.clone()).unwrap_or_default(),
		},
		Element::Select {
			id: "color".into(),
			label: "Color".into(),
			options: COLORS.iter().map(|(name, _)| (*name).to_owned()).collect(),
			value: tag
				.and_then(|tag| COLORS.iter().find(|(_, hex)| *hex == tag.color))
				.map_or(COLORS[0].0, |(name, _)| name)
				.to_owned(),
		},
		Element::Row {
			children: vec![
				Element::Button {
					id: "save".into(),
					label: "Save tag".into(),
				},
				Element::Button {
					id: "remove".into(),
					label: "Remove tag".into(),
				},
			],
		},
		Element::Text {
			text: format!("User ID {user}. Tags are only visible to you."),
		},
	]);
	panel
}

fn tag_user(input: &ExtensionInvocation) -> ExtensionOutput {
	let Some(MenuTarget::User { user_id, .. }) = &input.menu_target else {
		return ExtensionOutput::panel(vec![Element::Text {
			text: "Open this from a person's menu.".into(),
		}]);
	};
	let mut tags = load(input);
	let mut output = ExtensionOutput::default();
	match input.button() {
		Some("save") => match clean(input.value("text").unwrap_or_default()) {
			Ok(text) => {
				if !tags.contains_key(user_id) && tags.len() >= MAX_TAGS {
					output.output.output.panel = editor(
						user_id,
						tags.get(user_id),
						Some("You already have 200 tags."),
					);
					return output;
				}
				let color = input
					.value("color")
					.and_then(|name| COLORS.iter().find(|(label, _)| *label == name))
					.map_or(COLORS[0].1, |(_, hex)| hex)
					.to_owned();
				let tag = Tag { text, color };
				output.contribute(badge(user_id, &tag));
				output.contribute(composer_button());
				tags.insert(user_id.clone(), tag);
				save(&mut output, &tags);
				output.output.output.panel = editor(user_id, tags.get(user_id), Some("Saved."));
			}
			Err(reason) => {
				output.output.output.panel = editor(user_id, tags.get(user_id), Some(reason));
			}
		},
		Some("remove") => {
			tags.remove(user_id);
			output.contribute(ContributionOp::remove(
				Slot::UserBadge,
				format!("u{user_id}"),
			));
			save(&mut output, &tags);
			output.output.output.panel = editor(user_id, None, Some("Removed."));
		}
		_ => output.output.output.panel = editor(user_id, tags.get(user_id), None),
	}
	output
}

fn list(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut tags = load(input);
	let mut output = ExtensionOutput::default();
	if let Some(user) = input
		.button()
		.and_then(|button| button.strip_prefix("remove-"))
		&& tags.remove(user).is_some()
	{
		output.contribute(ContributionOp::remove(Slot::UserBadge, format!("u{user}")));
		save(&mut output, &tags);
	}
	let mut panel = vec![Element::Heading {
		text: "User tags".into(),
	}];
	if tags.is_empty() {
		panel.push(Element::Text {
			text: "No tags yet. Right-click a person and choose Tag this person.".into(),
		});
	}
	for (user, tag) in &tags {
		panel.push(Element::Row {
			children: vec![
				Element::Text {
					text: format!("{} — user {user}", tag.text),
				},
				Element::Button {
					id: format!("remove-{user}"),
					label: "Remove".into(),
				},
			],
		});
	}
	output.output.output.panel = panel;
	output
}

/// Runs when Harmony loads the extension: redraws every saved tag.
fn restore(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut output = ExtensionOutput::default();
	output.contribute(ContributionOp::Clear { slot: None });
	output.contribute(composer_button());
	for (user, tag) in load(input).iter().take(MAX_TAGS) {
		output.contribute(badge(user, tag));
	}
	output
}

pub fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	match input.action() {
		"tag-user" => tag_user(&input),
		"list" => list(&input),
		_ => restore(&input),
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::testing::TestHost;

	fn on_user(button: Option<&str>, text: &str, storage: Option<String>) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		let base = &mut input.invocation.invocation.invocation;
		base.action = "tag-user".into();
		base.values.insert("text".into(), text.into());
		base.values.insert("color".into(), "Green".into());
		base.storage = storage;
		input.button = button.map(Into::into);
		input.menu_target = Some(MenuTarget::User {
			user_id: "42".into(),
			guild_id: None,
			channel_id: None,
		});
		input
	}

	#[test]
	fn saving_a_tag_draws_it_and_activation_redraws_it_from_storage() {
		let mut host = TestHost::default();
		let output = host
			.run(on_user(Some("save"), "  Friend ", None), handle)
			.unwrap();
		let stored = output.output.output.storage.clone();
		let (target, item) = &host.contributions[&(Slot::UserBadge, "u42".into())];
		assert_eq!(*target, ContributionTarget::user("42"));
		assert_eq!(
			(item.text.as_str(), item.color.as_deref()),
			("Friend", Some("#3BA55C"))
		);
		assert!(
			host.contributions
				.contains_key(&(Slot::ComposerButton, "list".into()))
		);

		// A fresh session: activation alone rebuilds the badge and the button.
		let mut fresh = TestHost::default();
		let mut activation = ExtensionInvocation::default();
		activation.invocation.invocation.invocation.action = "restore".into();
		activation.invocation.invocation.invocation.storage = stored.clone();
		fresh.run(activation, handle).unwrap();
		assert_eq!(fresh.contributions, host.contributions);

		let output = host
			.run(on_user(Some("remove"), "", stored), handle)
			.unwrap();
		assert!(
			!host
				.contributions
				.contains_key(&(Slot::UserBadge, "u42".into()))
		);
		assert_eq!(output.output.output.storage.as_deref(), Some("{}"));
	}

	#[test]
	fn invalid_tags_are_refused_without_changes() {
		assert!(clean("   ").is_err());
		assert!(clean(&"x".repeat(25)).is_err());
		let mut host = TestHost::default();
		let output = host.run(on_user(Some("save"), "", None), handle).unwrap();
		assert!(host.contributions.is_empty());
		assert!(output.output.output.storage.is_none());
	}
}
