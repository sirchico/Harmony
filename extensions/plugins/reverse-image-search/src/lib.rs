//! Reverse Image Search: opens an image from a chosen message in a reverse image search
//! engine. Written for Harmony from the plugin's described behavior, using only the public
//! SDK: the `media_links` input, a choice setting and the `open_external_link` app action.
//! Nothing is fetched by the extension; the browser opens the engine with the image link.
use harmony_extension_sdk::{
	AppAction, Element, ExtensionInvocation, ExtensionOutput, HostEffect, MediaKind, MediaLink,
};

/// (setting value, label, URL prefix the percent-encoded image link is appended to).
pub const ENGINES: &[(&str, &str, &str)] = &[
	(
		"google",
		"Google Lens",
		"https://lens.google.com/uploadbyurl?url=",
	),
	(
		"bing",
		"Bing",
		"https://www.bing.com/images/search?view=detailv2&iss=sbi&q=imgurl:",
	),
	(
		"yandex",
		"Yandex",
		"https://yandex.com/images/search?rpt=imageview&url=",
	),
	("tineye", "TinEye", "https://tineye.com/search?url="),
	(
		"saucenao",
		"SauceNAO",
		"https://saucenao.com/search.php?url=",
	),
	("iqdb", "IQDB", "https://iqdb.org/?url="),
];
const MAX_LISTED: usize = 10;

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

/// The engine search URL for one image, or `None` for an unknown engine.
pub fn search_url(engine: &str, image: &str) -> Option<String> {
	let (_, _, prefix) = ENGINES.iter().find(|(id, ..)| *id == engine)?;
	Some(format!("{prefix}{}", percent_encode(image)))
}

fn images(input: &ExtensionInvocation) -> Vec<&MediaLink> {
	input
		.media
		.iter()
		.filter(|link| link.kind == MediaKind::Image)
		.take(MAX_LISTED)
		.collect()
}

fn label(engine: &str) -> &str {
	ENGINES
		.iter()
		.find(|(id, ..)| *id == engine)
		.map_or(engine, |(_, label, _)| label)
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	// A panel choice overrides the saved default for this search only.
	let engine = input
		.value("engine")
		.and_then(|label| ENGINES.iter().find(|(_, name, _)| *name == label))
		.map(|(id, ..)| *id)
		.or(input.setting_text("engine"))
		.unwrap_or("google")
		.to_owned();
	let images = images(&input);
	let mut panel = vec![Element::Heading {
		text: "Reverse image search".into(),
	}];
	if images.is_empty() {
		panel.push(Element::Text {
			text: "This message has no images Harmony can link to.".into(),
		});
		return ExtensionOutput::panel(panel);
	}
	let chosen = input
		.button()
		.and_then(|button| button.strip_prefix("image-"))
		.and_then(|index| index.parse::<usize>().ok())
		.filter(|index| *index < images.len())
		.unwrap_or(0);
	panel.push(Element::Select {
		id: "engine".into(),
		label: "Search engine".into(),
		options: ENGINES
			.iter()
			.map(|(_, name, _)| (*name).to_owned())
			.collect(),
		value: label(&engine).to_owned(),
	});
	for (index, image) in images.iter().enumerate() {
		let name = image
			.filename
			.clone()
			.unwrap_or_else(|| format!("Image {}", index + 1));
		let size = match (image.width, image.height) {
			(Some(width), Some(height)) => format!(" ({width}×{height})"),
			_ => String::new(),
		};
		if index == chosen {
			panel.push(Element::Text {
				text: format!("Selected: {name}{size}"),
			});
		} else {
			panel.push(Element::Button {
				id: format!("image-{index}"),
				label: format!("Use {name}{size}"),
			});
		}
	}
	let mut output = ExtensionOutput::panel(panel);
	if let Some(url) = search_url(&engine, &images[chosen].url) {
		output.output.effects.push(HostEffect::AppAction {
			action: AppAction::OpenExternalLink { url },
		});
	}
	output
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{MediaSource, SettingValue, testing::TestHost};

	fn image(name: &str) -> MediaLink {
		MediaLink {
			source: MediaSource::Attachment,
			kind: MediaKind::Image,
			url: format!("https://cdn.discordapp.com/attachments/1/2/{name}?ex=1&is=2"),
			attachment_id: Some("2".into()),
			filename: Some(name.into()),
			content_type: Some("image/png".into()),
			size: Some(10),
			width: Some(4),
			height: Some(3),
		}
	}

	#[test]
	fn the_saved_engine_opens_the_chosen_image_with_its_query_encoded() {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "search".into();
		input.button = Some("image-1".into());
		input.media = vec![image("a.png"), image("b.png")];
		input
			.settings
			.insert("engine".into(), SettingValue::Text("tineye".into()));
		let output = TestHost::default().run(input, handle).unwrap();
		assert_eq!(
			output.output.effects,
			[HostEffect::AppAction {
				action: AppAction::OpenExternalLink {
					url: "https://tineye.com/search?url=https%3A%2F%2Fcdn.discordapp.com%2Fattachments%2F1%2F2%2Fb.png%3Fex%3D1%26is%3D2".into()
				}
			}]
		);
	}
}
