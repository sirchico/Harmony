//! Copy File Contents: a copy button on text file attachments. Clicking it reads the file and
//! copies its text at once; the button shows a check mark. Written for Harmony using only the
//! public API v2 SDK: an `attachment` action with a file filter and an `attachment_read`
//! host request answered between rounds.
use harmony_extension_sdk::{
	ContentEncoding, Element, ExtensionInvocation, ExtensionOutput, HostEffect, HostRequest,
	MAX_COPY_TEXT_BYTES_V2, ResponseValue,
};

const READ: &str = "file";

fn note(text: impl Into<String>) -> ExtensionOutput {
	ExtensionOutput::panel(vec![
		Element::Heading {
			text: "Copy File Contents".into(),
		},
		Element::Text { text: text.into() },
	])
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let Some(file) = input.media.first() else {
		return note("Use the copy button on a text file.");
	};
	let Some(response) = input.response(READ) else {
		let Some(attachment_id) = file.attachment_id.clone() else {
			return note("Only files attached to a message can be copied.");
		};
		let mut output = ExtensionOutput::default();
		output.request(HostRequest::AttachmentRead {
			id: READ.into(),
			attachment_id,
			max_bytes: Some(MAX_COPY_TEXT_BYTES_V2 as u32),
		});
		return output;
	};
	match (&response.value, &response.error) {
		(Some(ResponseValue::Attachment { content }), _) => {
			if content.encoding != ContentEncoding::Text {
				note("This file is not text, so there is nothing to copy.")
			} else if content.truncated {
				note("File too large to copy.")
			} else {
				ExtensionOutput::effect(HostEffect::CopyText {
					text: content.data.clone(),
				})
			}
		}
		(_, Some(error)) => note(format!("Could not read the file: {}", error.message)),
		_ => note("Harmony returned an unexpected answer."),
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{MediaKind, MediaLink, MediaSource, testing::TestHost};

	fn on_file(id: &str) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "copy".into();
		input.media = vec![MediaLink {
			source: MediaSource::Attachment,
			kind: MediaKind::File,
			url: format!("https://cdn.discordapp.com/attachments/1/{id}/notes.txt"),
			attachment_id: Some(id.into()),
			filename: Some("notes.txt".into()),
			content_type: Some("text/plain".into()),
			size: Some(5),
			width: None,
			height: None,
		}];
		input
	}

	#[test]
	fn the_button_copies_whole_text_files_and_explains_anything_else() {
		let mut host = TestHost::default();
		host.attachment("2", "notes.txt", Some("text/plain"), b"hello");
		host.attachment("3", "blob.bin", None, &[0xff, 0xfe, 0x00]);
		host.attachment(
			"4",
			"big.txt",
			None,
			&vec![b'x'; MAX_COPY_TEXT_BYTES_V2 + 1],
		);
		let copied = host.run(on_file("2"), handle).unwrap();
		assert_eq!(
			copied.output.effects,
			[HostEffect::CopyText {
				text: "hello".into()
			}]
		);
		for id in ["3", "4", "9"] {
			let output = host.run(on_file(id), handle).unwrap();
			assert!(output.output.effects.is_empty(), "{id}");
			assert!(!output.output.output.panel.is_empty(), "{id}");
		}
	}
}
