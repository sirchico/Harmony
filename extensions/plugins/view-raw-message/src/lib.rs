//! ViewRaw: a hover-toolbar button that shows a message's raw text and data in a window,
//! with buttons to copy either. Written for Harmony using only the public API v2 SDK. It sees
//! only the message text and public IDs Harmony passes to `selected_message` actions.
use harmony_extension_sdk::{
	Element, ExtensionInvocation, ExtensionOutput, HostEffect, MAX_COPY_TEXT_BYTES_V2, serde_json,
};

/// Text shown per panel element; long messages are split over several.
const CHUNK: usize = 4000;
/// Most text shown in the window; copying still takes everything up to the copy limit.
const SHOWN: usize = 64 * 1024;

fn copy(text: String) -> ExtensionOutput {
	ExtensionOutput::effect(HostEffect::CopyText { text })
}

/// The message's public data as pretty JSON with sorted keys.
pub fn message_data(input: &ExtensionInvocation, content: &str) -> String {
	let mut data = serde_json::Map::new();
	data.insert("content".into(), content.into());
	if let Some(message) = &input.selected_message_ref {
		data.insert("author_id".into(), message.author_id.clone().into());
		data.insert("channel_id".into(), message.channel_id.clone().into());
		if let Some(guild) = &message.guild_id {
			data.insert("guild_id".into(), guild.clone().into());
		}
		data.insert("id".into(), message.message_id.clone().into());
	}
	serde_json::to_string_pretty(&serde_json::Value::Object(data)).unwrap_or_default()
}

/// Splits text into display chunks on character boundaries, up to the display limit.
pub fn chunks(text: &str) -> (Vec<&str>, usize) {
	let mut rest = text;
	let mut out = Vec::new();
	let mut shown = 0;
	while !rest.is_empty() && shown < SHOWN {
		let mut end = rest.len().min(CHUNK);
		while !rest.is_char_boundary(end) {
			end -= 1;
		}
		out.push(&rest[..end]);
		shown += end;
		rest = &rest[end..];
	}
	(out, rest.len())
}

fn window(content: &str, data: &str) -> ExtensionOutput {
	let mut panel = vec![
		Element::Heading {
			text: "Raw Message Data".into(),
		},
		Element::Heading {
			text: "Message Content".into(),
		},
	];
	if content.is_empty() {
		panel.push(Element::Text {
			text: "(no text)".into(),
		});
	}
	let (shown, omitted) = chunks(content);
	panel.extend(
		shown
			.into_iter()
			.map(|text| Element::Text { text: text.into() }),
	);
	if omitted > 0 {
		panel.push(Element::Text {
			text: format!("{omitted} more bytes are not shown."),
		});
	}
	panel.push(Element::Separator);
	panel.push(Element::Heading {
		text: "Message Data".into(),
	});
	panel.push(Element::Text { text: data.into() });
	panel.push(Element::Row {
		children: vec![
			Element::Button {
				id: "copy-data".into(),
				label: "Copy Message Data".into(),
			},
			Element::Button {
				id: "copy-content".into(),
				label: "Copy Raw Content".into(),
			},
		],
	});
	ExtensionOutput::panel(panel)
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let content = input.selected_message().unwrap_or_default().to_owned();
	let data = message_data(&input, &content);
	let copy_mode = input.setting_text("click") == Some("copy");
	let wanted = match input.button() {
		Some("copy-content") => Some(content.clone()),
		Some("copy-data") => Some(data.clone()),
		_ if copy_mode => Some(content.clone()),
		_ => None,
	};
	match wanted {
		Some(text) if text.is_empty() => ExtensionOutput::panel(vec![Element::Text {
			text: "This message has no text to copy.".into(),
		}]),
		Some(text) if text.len() <= MAX_COPY_TEXT_BYTES_V2 => copy(text),
		Some(_) => ExtensionOutput::panel(vec![Element::Text {
			text: "This message is too long to copy.".into(),
		}]),
		None => window(&content, &data),
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{SelectedMessageRef, SettingValue, testing::TestHost};

	fn on_message(text: &str, button: Option<&str>, click: &str) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "view-raw".into();
		input.invocation.invocation.invocation.selected_message = Some(text.into());
		input.selected_message_ref = Some(SelectedMessageRef {
			channel_id: "2".into(),
			message_id: "3".into(),
			author_id: "4".into(),
			guild_id: None,
		});
		input.button = button.map(Into::into);
		input
			.settings
			.insert("click".into(), SettingValue::Text(click.into()));
		input
	}

	#[test]
	fn the_window_shows_content_and_sorted_data_and_buttons_copy_each() {
		let window = TestHost::default()
			.run(on_message("**hi**", None, "window"), handle)
			.unwrap();
		let texts: Vec<_> = window
			.output
			.output
			.panel
			.iter()
			.filter_map(|element| match element {
				Element::Text { text } => Some(text.as_str()),
				_ => None,
			})
			.collect();
		assert_eq!(texts[0], "**hi**");
		assert!(texts[1].find("\"author_id\"") < texts[1].find("\"id\""));
		let data = TestHost::default()
			.run(on_message("**hi**", Some("copy-data"), "window"), handle)
			.unwrap();
		assert!(
			matches!(&data.output.effects[..], [HostEffect::CopyText { text }] if text.contains("\"channel_id\": \"2\""))
		);
		let quick = TestHost::default()
			.run(on_message("**hi**", None, "copy"), handle)
			.unwrap();
		assert_eq!(
			quick.output.effects,
			[HostEffect::CopyText {
				text: "**hi**".into()
			}]
		);
		let empty = TestHost::default()
			.run(on_message("", Some("copy-content"), "window"), handle)
			.unwrap();
		assert!(empty.output.effects.is_empty());
	}

	#[test]
	fn long_text_is_split_on_character_boundaries_and_capped() {
		let text = "é".repeat(SHOWN);
		let (shown, omitted) = chunks(&text);
		assert!(shown.iter().all(|chunk| chunk.len() <= CHUNK));
		assert_eq!(
			shown.iter().map(|chunk| chunk.len()).sum::<usize>() + omitted,
			text.len()
		);
		assert!(omitted > 0);
	}
}
