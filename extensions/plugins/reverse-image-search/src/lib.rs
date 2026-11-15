//! Reverse Image Search: a "Search Image" submenu on images that opens the image in a reverse
//! image search engine, or in all of them. Written for Harmony using only the public API v2
//! SDK (an `image` action with choices). Nothing is fetched by the extension; the engine
//! receives the image's address in the browser.
use harmony_extension_sdk::{AppAction, Element, ExtensionInvocation, ExtensionOutput, HostEffect};

/// (choice ID, URL prefix the percent-encoded image address is appended to).
pub const ENGINES: &[(&str, &str)] = &[
	("google", "https://lens.google.com/uploadbyurl?url="),
	(
		"yandex",
		"https://yandex.com/images/search?rpt=imageview&url=",
	),
	("saucenao", "https://saucenao.com/search.php?url="),
	("iqdb", "https://iqdb.org/?url="),
	(
		"bing",
		"https://www.bing.com/images/search?view=detailv2&iss=sbi&q=imgurl:",
	),
	("tineye", "https://www.tineye.com/search?url="),
	("imgops", "https://imgops.com/start?url="),
];

fn percent_encode(text: &str) -> String {
	let mut out = String::with_capacity(text.len() * 3);
	for byte in text.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
			out.push(byte as char);
		} else {
			out.push_str(&format!("%{byte:02X}"));
		}
	}
	out
}

/// The search page for one engine, or `None` for an unknown engine.
pub fn search_url(engine: &str, image: &str) -> Option<String> {
	let (_, prefix) = ENGINES.iter().find(|(id, _)| *id == engine)?;
	Some(format!("{prefix}{}", percent_encode(image)))
}

fn open(url: String) -> HostEffect {
	HostEffect::AppAction {
		action: AppAction::OpenExternalLink { url },
	}
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let Some(image) = input.media.first() else {
		return ExtensionOutput::panel(vec![Element::Text {
			text: "Right-click an image and choose Search Image.".into(),
		}]);
	};
	let mut output = ExtensionOutput::default();
	match input.button().unwrap_or("google") {
		"all" => {
			for (engine, _) in ENGINES {
				output
					.output
					.effects
					.extend(search_url(engine, &image.url).map(open));
			}
		}
		engine => output
			.output
			.effects
			.extend(search_url(engine, &image.url).map(open)),
	}
	output
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{MediaKind, MediaLink, MediaSource, testing::TestHost};

	fn on_image(button: &str) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "search".into();
		input.button = Some(button.into());
		input.media = vec![MediaLink {
			source: MediaSource::Attachment,
			kind: MediaKind::Image,
			url: "https://cdn.discordapp.com/attachments/1/2/b.png?ex=1&is=2".into(),
			attachment_id: Some("2".into()),
			filename: Some("b.png".into()),
			content_type: Some("image/png".into()),
			size: Some(10),
			width: Some(4),
			height: Some(3),
		}];
		input
	}

	#[test]
	fn each_engine_opens_the_image_address_encoded_and_all_opens_every_engine() {
		let one = TestHost::default().run(on_image("tineye"), handle).unwrap();
		assert_eq!(
			one.output.effects,
			[open(
				"https://www.tineye.com/search?url=https%3A%2F%2Fcdn.discordapp.com%2Fattachments%2F1%2F2%2Fb.png%3Fex%3D1%26is%3D2"
					.into()
			)]
		);
		let all = TestHost::default().run(on_image("all"), handle).unwrap();
		assert_eq!(all.output.effects.len(), ENGINES.len());
		assert!(
			TestHost::default()
				.run(on_image("unknown"), handle)
				.unwrap()
				.output
				.effects
				.is_empty()
		);
	}
}
