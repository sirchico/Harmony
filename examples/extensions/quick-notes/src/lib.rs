//! Quick Notes: an example API v2 extension built only from the public SDK, the way an
//! independent author would. It saves message snippets to the host's key-value store and
//! lists them over several request rounds, with host-built settings.
//!
//! * `save` (message action): reads the next note number, writes the note, trims the oldest.
//! * `show` (panel action): lists note keys, reads them, shows each with a delete button.
//! * `delete-<number>` (panel button): deletes one note in the same round that lists again.
use harmony_extension_sdk::{
	Element, ExtensionInvocation, ExtensionOutput, HostRequest, ResponseValue,
};

const PREFIX: &str = "note/";
const NEXT: &str = "next";

fn keys(input: &ExtensionInvocation, id: &str) -> Vec<String> {
	match input
		.response(id)
		.and_then(|response| response.value.as_ref())
	{
		Some(ResponseValue::KvKeys { keys, .. }) => keys.clone(),
		_ => Vec::new(),
	}
}

fn values(input: &ExtensionInvocation, id: &str) -> Vec<(String, Option<String>)> {
	match input
		.response(id)
		.and_then(|response| response.value.as_ref())
	{
		Some(ResponseValue::Kv { values }) => values.clone().into_iter().collect(),
		_ => Vec::new(),
	}
}

fn snippet(text: &str, limit: usize) -> String {
	let mut out: String = text.chars().take(limit).collect();
	if out.len() < text.len() {
		out.push('…');
	}
	out
}

fn save(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut output = ExtensionOutput::default();
	if input.round == 0 {
		output.request(HostRequest::KvGet {
			id: "next".into(),
			keys: vec![NEXT.into()],
		});
		output.request(HostRequest::KvList {
			id: "notes".into(),
			prefix: PREFIX.into(),
			limit: None,
		});
		return output;
	}
	let next: u64 = values(input, "next")
		.into_iter()
		.find_map(|(_, value)| value?.parse().ok())
		.unwrap_or(1);
	let max = input.setting_number("max-notes").unwrap_or(20).clamp(1, 50) as usize;
	let mut text = snippet(input.selected_message().unwrap_or_default(), 500);
	if input.setting_bool("with-link")
		&& let Some(message) = &input.selected_message_ref
	{
		let guild = message.guild_id.as_deref().unwrap_or("@me");
		text.push_str(&format!(
			"\nhttps://discord.com/channels/{guild}/{}/{}",
			message.channel_id, message.message_id
		));
	}
	output.kv_set(format!("{PREFIX}{next:08}"), text);
	output.kv_set(NEXT, (next + 1).to_string());
	// Keys sort by number, so the oldest come first.
	let existing = keys(input, "notes");
	for old in existing
		.iter()
		.take((existing.len() + 1).saturating_sub(max))
	{
		output.kv_delete(old.clone());
	}
	output.output.output.panel = vec![Element::Text {
		text: format!("Saved note {next}. Open Quick Notes from the tools menu to see it."),
	}];
	output
}

fn show(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut output = ExtensionOutput::default();
	// Round 0 lists keys (after any delete in the same round); round 1 reads them.
	if input.round == 0 {
		if let Some(number) = input
			.button()
			.and_then(|button| button.strip_prefix("delete-"))
			&& !number.is_empty()
			&& number.bytes().all(|b| b.is_ascii_digit())
		{
			output.kv_delete(format!("{PREFIX}{number}"));
		}
		output.request(HostRequest::KvList {
			id: "notes".into(),
			prefix: PREFIX.into(),
			limit: None,
		});
		return output;
	}
	if input.round == 1 {
		let keys = keys(input, "notes");
		if !keys.is_empty() {
			output.request(HostRequest::KvGet {
				id: "values".into(),
				keys: keys.into_iter().rev().take(64).collect(),
			});
			return output;
		}
	}
	let mut panel = vec![Element::Heading {
		text: "Quick notes".into(),
	}];
	let notes = values(input, "values");
	if notes.is_empty() {
		panel.push(Element::Text {
			text: "No notes yet. Use Save to Quick Notes on any message.".into(),
		});
	}
	let mut notes: Vec<_> = notes
		.into_iter()
		.filter_map(|(key, value)| Some((key, value?)))
		.collect();
	notes.reverse();
	for (index, (key, text)) in notes.into_iter().enumerate() {
		if index > 0 {
			panel.push(Element::Separator);
		}
		panel.push(Element::Text { text });
		panel.push(Element::Button {
			id: format!("delete-{}", key.trim_start_matches(PREFIX)),
			label: "Delete".into(),
		});
	}
	if let Some(host) = input.host()
		&& !host.supports_feature("sync", 1, 0)
	{
		panel.push(Element::Separator);
		panel.push(Element::Text {
			text: "Notes stay on this device; this Harmony has no sync feature.".into(),
		});
	}
	output.output.output.panel = panel;
	output
}

pub fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	if input.action() == "save" {
		save(&input)
	} else {
		show(&input)
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{SelectedMessageRef, SettingValue, testing::TestHost};

	fn saving(text: &str) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "save".into();
		input.invocation.invocation.invocation.selected_message = Some(text.into());
		input.selected_message_ref = Some(SelectedMessageRef {
			channel_id: "2".into(),
			message_id: "3".into(),
			author_id: "4".into(),
			guild_id: None,
		});
		input
			.settings
			.insert("max-notes".into(), SettingValue::Number(2));
		input
			.settings
			.insert("with-link".into(), SettingValue::Bool(true));
		input
	}

	#[test]
	fn saving_keeps_the_newest_notes_and_delete_lists_again() {
		let mut host = TestHost::default();
		for text in ["one", "two", "three"] {
			host.run(saving(text), handle).unwrap();
		}
		assert_eq!(
			host.kv.keys().collect::<Vec<_>>(),
			["next", "note/00000002", "note/00000003"]
		);
		assert!(host.kv["note/00000003"].ends_with("/channels/@me/2/3"));
		let mut delete = ExtensionInvocation::default();
		delete.invocation.invocation.invocation.action = "show".into();
		delete.button = Some("delete-00000003".into());
		let output = host.run(delete, handle).unwrap();
		let texts: Vec<_> = output
			.output
			.output
			.panel
			.iter()
			.filter_map(|element| match element {
				Element::Text { text } => Some(text.as_str()),
				_ => None,
			})
			.collect();
		assert_eq!(texts.len(), 1);
		assert!(texts[0].starts_with("two"));
	}
}
