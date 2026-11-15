//! Copy File Contents: reads a text attachment on a chosen message and offers to copy it.
//! Written for Harmony from the plugin's described behavior, using only the public SDK: the
//! `media_links` input to find attachments, an `attachment_read` host request answered between
//! rounds, a number setting, and the `copy_text` effect, which v2 allows up to 64 KiB.
use harmony_extension_sdk::{
	ContentEncoding, Element, ExtensionInvocation, ExtensionOutput, HostEffect, HostRequest,
	MAX_COPY_TEXT_BYTES_V2, MediaKind, MediaLink, ResponseValue,
};

const READ: &str = "file";
const MAX_LISTED: usize = 10;
const PREVIEW_BYTES: usize = 3000;

const TEXT_EXTENSIONS: &[&str] = &[
	"txt", "md", "log", "json", "toml", "yaml", "yml", "xml", "csv", "tsv", "ini", "cfg", "conf",
	"rs", "py", "js", "ts", "tsx", "jsx", "c", "h", "cpp", "hpp", "cs", "java", "kt", "go", "rb",
	"php", "lua", "sh", "ps1", "bat", "sql", "html", "css", "scss", "diff", "patch", "srt", "env",
];

/// Whether an attachment is likely text, from its MIME type or file extension.
pub fn texty(link: &MediaLink) -> bool {
	let mime = link.content_type.as_deref().unwrap_or("");
	let extension = link
		.filename
		.as_deref()
		.and_then(|name| name.rsplit_once('.'))
		.map(|(_, extension)| extension.to_ascii_lowercase());
	mime.starts_with("text/")
		|| ["json", "xml", "yaml", "toml", "javascript", "x-sh", "sql"]
			.iter()
			.any(|kind| mime.contains(kind))
		|| extension.is_some_and(|extension| TEXT_EXTENSIONS.contains(&extension.as_str()))
}

fn files(input: &ExtensionInvocation) -> Vec<&MediaLink> {
	input
		.media
		.iter()
		.filter(|link| link.attachment_id.is_some() && link.kind == MediaKind::File)
		.filter(|link| texty(link))
		.take(MAX_LISTED)
		.collect()
}

fn preview(text: &str) -> String {
	if text.len() <= PREVIEW_BYTES {
		return text.to_owned();
	}
	let mut end = PREVIEW_BYTES;
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	format!("{}\n…", &text[..end])
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let files = files(&input);
	let mut panel = vec![Element::Heading {
		text: "Copy file contents".into(),
	}];
	if files.is_empty() {
		panel.push(Element::Text {
			text: "This message has no text attachments.".into(),
		});
		return ExtensionOutput::panel(panel);
	}
	let chosen = input
		.continuation
		.as_deref()
		.or_else(|| {
			input
				.button()
				.and_then(|button| button.strip_prefix("file-"))
		})
		.and_then(|index| index.parse::<usize>().ok())
		.filter(|index| *index < files.len())
		.unwrap_or(0);
	let Some(response) = input.response(READ) else {
		let limit = input
			.setting_number("max-kib")
			.unwrap_or(64)
			.clamp(1, (MAX_COPY_TEXT_BYTES_V2 / 1024) as i64) as u32
			* 1024;
		let mut output = ExtensionOutput::default();
		output.request(HostRequest::AttachmentRead {
			id: READ.into(),
			attachment_id: files[chosen].attachment_id.clone().unwrap_or_default(),
			max_bytes: Some(limit),
		});
		// Which file this round asked for, so the answer is matched to it.
		output.continuation = Some(chosen.to_string());
		return output;
	};
	let mut copy = None;
	match (&response.value, &response.error) {
		(Some(ResponseValue::Attachment { content }), _) => {
			panel.push(Element::Text {
				text: format!(
					"{} · {:.1} KiB",
					content.filename,
					content.size as f64 / 1024.0
				),
			});
			if content.encoding == ContentEncoding::Text {
				if content.truncated {
					panel.push(Element::Text {
						text: format!(
							"Only the first {:.1} KiB was read; raise the limit in this extension's settings to copy more.",
							content.data.len() as f64 / 1024.0
						),
					});
				}
				panel.push(Element::Separator);
				panel.push(Element::Text {
					text: preview(&content.data),
				});
				copy = Some(content.data.clone());
			} else {
				panel.push(Element::Text {
					text: "This file is not UTF-8 text, so there is nothing to copy as text."
						.into(),
				});
			}
		}
		(_, Some(error)) => panel.push(Element::Text {
			text: format!("Could not read the file: {}", error.message),
		}),
		_ => panel.push(Element::Text {
			text: "Harmony returned an unexpected answer.".into(),
		}),
	}
	if files.len() > 1 {
		panel.push(Element::Separator);
		for (index, file) in files.iter().enumerate() {
			if index != chosen {
				panel.push(Element::Button {
					id: format!("file-{index}"),
					label: format!(
						"Show {}",
						file.filename.as_deref().unwrap_or("another file")
					),
				});
			}
		}
	}
	let mut output = ExtensionOutput::panel(panel);
	if let Some(text) = copy.filter(|text| !text.is_empty() && text.len() <= MAX_COPY_TEXT_BYTES_V2)
	{
		output.output.effects.push(HostEffect::CopyText { text });
	}
	output
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{MediaSource, testing::TestHost};

	fn file(id: &str, name: &str, mime: Option<&str>) -> MediaLink {
		MediaLink {
			source: MediaSource::Attachment,
			kind: MediaKind::File,
			url: format!("https://cdn.discordapp.com/attachments/1/{id}/{name}"),
			attachment_id: Some(id.into()),
			filename: Some(name.into()),
			content_type: mime.map(Into::into),
			size: Some(5),
			width: None,
			height: None,
		}
	}

	#[test]
	fn reads_the_chosen_text_file_and_skips_binaries() {
		let mut host = TestHost::default();
		host.attachment("2", "notes.txt", Some("text/plain"), b"first");
		host.attachment("3", "main.rs", None, b"fn main() {}");
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "copy".into();
		input.button = Some("file-1".into());
		input.media = vec![
			file("2", "notes.txt", Some("text/plain")),
			file("9", "archive.zip", Some("application/zip")),
			file("3", "main.rs", None),
		];
		let output = host.run(input, handle).unwrap();
		assert_eq!(
			output.output.effects,
			[HostEffect::CopyText {
				text: "fn main() {}".into()
			}]
		);
		assert!(matches!(
			&host.inputs[0].media[..],
			[_, zip, _] if zip.filename.as_deref() == Some("archive.zip")
		));
	}
}
