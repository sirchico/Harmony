//! Base64 Decoder: finds Base64 runs in a chosen message and shows what they decode to, and
//! encodes the draft on request. Written for Harmony from the plugin's described behavior.
//! Uses only the public SDK: a `message` action, a `composer` action, settings and `copy_text`.
use harmony_extension_sdk::{
	Element, ExtensionInvocation, ExtensionOutput, HostEffect, MAX_COPY_TEXT_BYTES_V2,
};

/// Decoded results shown at once; the panel stays small on messages full of tokens.
const MAX_RESULTS: usize = 8;
/// Shown per result, in bytes; the copy effect carries the full first result.
const MAX_SHOWN: usize = 2000;

fn value(byte: u8, url_safe: bool) -> Option<u8> {
	Some(match byte {
		b'A'..=b'Z' => byte - b'A',
		b'a'..=b'z' => byte - b'a' + 26,
		b'0'..=b'9' => byte - b'0' + 52,
		b'+' => 62,
		b'/' => 63,
		b'-' if url_safe => 62,
		b'_' if url_safe => 63,
		_ => return None,
	})
}

/// Decodes standard or URL-safe Base64, with or without padding.
pub fn decode(text: &str, url_safe: bool) -> Option<Vec<u8>> {
	let trimmed = text.trim_end_matches('=');
	if text.len() - trimmed.len() > 2 || trimmed.len() % 4 == 1 {
		return None;
	}
	let mut out = Vec::with_capacity(trimmed.len() * 3 / 4);
	let (mut buffer, mut bits) = (0u32, 0u32);
	for byte in trimmed.bytes() {
		buffer = buffer << 6 | u32::from(value(byte, url_safe)?);
		bits += 6;
		if bits >= 8 {
			bits -= 8;
			out.push((buffer >> bits) as u8);
		}
	}
	// Leftover bits must be zero, or the run is not canonical Base64.
	(buffer & ((1 << bits) - 1) == 0).then_some(out)
}

pub fn encode(bytes: &[u8]) -> String {
	const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
	let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
	for chunk in bytes.chunks(3) {
		let n = u32::from(chunk[0]) << 16
			| u32::from(*chunk.get(1).unwrap_or(&0)) << 8
			| u32::from(*chunk.get(2).unwrap_or(&0));
		for index in 0..4 {
			out.push(if index <= chunk.len() {
				TABLE[(n >> (18 - 6 * index) & 63) as usize] as char
			} else {
				'='
			});
		}
	}
	out
}

/// Runs of Base64 characters at least `min` long that decode to readable UTF-8 text.
pub fn find(text: &str, min: usize, url_safe: bool) -> Vec<(String, String)> {
	let mut found = Vec::new();
	let token = |c: char| {
		c.is_ascii_alphanumeric()
			|| matches!(c, '+' | '/' | '=')
			|| url_safe && matches!(c, '-' | '_')
	};
	for run in text.split(|c: char| !token(c)) {
		if run.len() < min || found.len() == MAX_RESULTS {
			continue;
		}
		// Plain words are valid Base64 too; insist on a mix that real encodings show.
		let mixed = run
			.bytes()
			.any(|b| b.is_ascii_digit() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_'))
			|| run.bytes().any(|b| b.is_ascii_uppercase())
				&& run.bytes().any(|b| b.is_ascii_lowercase());
		if !mixed {
			continue;
		}
		let Some(bytes) = decode(run, url_safe) else {
			continue;
		};
		let Ok(decoded) = String::from_utf8(bytes) else {
			continue;
		};
		if decoded.is_empty()
			|| decoded
				.chars()
				.any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
		{
			continue;
		}
		found.push((run.to_owned(), decoded));
	}
	found
}

fn shown(text: &str) -> String {
	if text.len() <= MAX_SHOWN {
		return text.to_owned();
	}
	let mut end = MAX_SHOWN;
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	format!("{}…", &text[..end])
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let url_safe = input.setting_bool("url-safe");
	let min = input
		.setting_number("min-length")
		.unwrap_or(8)
		.clamp(4, 256) as usize;
	match input.action() {
		"encode" => {
			let draft = input
				.invocation
				.invocation
				.invocation
				.composer
				.clone()
				.unwrap_or_default();
			let mut output = ExtensionOutput::default();
			if draft.is_empty() {
				output.output.output.panel = vec![Element::Text {
					text: "The draft is empty; type something to encode first.".into(),
				}];
			} else {
				output.output.output.replacement = Some(encode(draft.as_bytes()));
			}
			output
		}
		_ => {
			let message = input.selected_message().unwrap_or_default();
			let found = find(message, min, url_safe);
			let mut panel = vec![Element::Heading {
				text: "Base64 decoder".into(),
			}];
			if found.is_empty() {
				panel.push(Element::Text {
					text: format!(
						"No Base64 text of at least {min} characters that decodes to readable text."
					),
				});
				return ExtensionOutput::panel(panel);
			}
			for (index, (encoded, decoded)) in found.iter().enumerate() {
				if index > 0 {
					panel.push(Element::Separator);
				}
				panel.push(Element::Text {
					text: format!("Encoded: {}", shown(encoded)),
				});
				panel.push(Element::Text {
					text: format!("Decoded: {}", shown(decoded)),
				});
			}
			let mut output = ExtensionOutput::panel(panel);
			let first = &found[0].1;
			if first.len() <= MAX_COPY_TEXT_BYTES_V2 {
				output.output.effects.push(HostEffect::CopyText {
					text: first.clone(),
				});
			}
			output
		}
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{SettingValue, testing::TestHost};

	#[test]
	fn decodes_padded_unpadded_and_url_safe_runs_but_not_plain_words() {
		assert_eq!(decode("aGVsbG8=", false).unwrap(), b"hello");
		assert_eq!(decode("aGVsbG8", false).unwrap(), b"hello");
		assert_eq!(decode("-_8", true).unwrap(), [0xfb, 0xff]);
		assert!(decode("-_8", false).is_none());
		assert!(decode("aGVsbG9=", false).is_none(), "non-zero padding bits");
		let found = find("see aGVsbG8gd29ybGQ= and Mississippi", 8, false);
		assert_eq!(found, [("aGVsbG8gd29ybGQ=".into(), "hello world".into())]);
		assert_eq!(encode(b"hello world"), "aGVsbG8gd29ybGQ=");
	}

	#[test]
	fn message_action_shows_results_and_offers_to_copy_the_first() {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "decode".into();
		input.invocation.invocation.invocation.selected_message =
			Some("x aGVsbG8gd29ybGQ= y".into());
		input
			.settings
			.insert("min-length".into(), SettingValue::Number(8));
		let output = TestHost::default().run(input, handle).unwrap();
		assert_eq!(
			output.output.effects,
			[HostEffect::CopyText {
				text: "hello world".into()
			}]
		);
	}
}
