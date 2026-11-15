//! External Search Tools: translate or search a message's text in the browser. A Translate
//! button sits in the message hover toolbar, and both tools are in the message menu. Written
//! for Harmony using only the public SDK; the extension builds a fixed HTTPS address and the
//! browser does the rest. Preferences are three allowlisted values in local storage.
use harmony_extension_sdk::{
	AppAction, Element, ExtensionInvocation, ExtensionOutput, HostEffect, serde_json,
};
use serde::{Deserialize, Serialize};

/// Longest message text or query put in an address, in bytes.
const MAX_QUERY: usize = 1024;

pub const PROVIDERS: &[(&str, &str)] = &[("google", "Google Translate"), ("deepl", "DeepL")];
pub const LANGUAGES: &[(&str, &str)] = &[
	("en", "English"),
	("cs", "Czech"),
	("de", "German"),
	("es", "Spanish"),
	("fr", "French"),
	("it", "Italian"),
	("ja", "Japanese"),
	("pl", "Polish"),
	("pt", "Portuguese"),
	("ru", "Russian"),
	("tr", "Turkish"),
	("uk", "Ukrainian"),
	("zh", "Chinese"),
];
pub const ENGINES: &[(&str, &str)] = &[
	("duckduckgo", "DuckDuckGo"),
	("google", "Google"),
	("brave", "Brave"),
	("bing", "Bing"),
	("wikipedia", "Wikipedia"),
];

/// Saved preferences; field names match what earlier versions stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
	pub provider: String,
	pub target_language: String,
	pub search_engine: String,
}

impl Default for Preferences {
	fn default() -> Self {
		Self {
			provider: "google".into(),
			target_language: "en".into(),
			search_engine: "duckduckgo".into(),
		}
	}
}

fn known(list: &[(&str, &str)], value: &str) -> bool {
	list.iter().any(|(id, _)| *id == value)
}

impl Preferences {
	fn valid(&self) -> bool {
		known(PROVIDERS, &self.provider)
			&& known(LANGUAGES, &self.target_language)
			&& known(ENGINES, &self.search_engine)
	}
	/// Saved preferences, or the defaults when nothing valid is saved.
	pub fn load(storage: Option<&str>) -> Self {
		storage
			.and_then(|text| serde_json::from_str::<Self>(text).ok())
			.filter(Self::valid)
			.unwrap_or_default()
	}
}

/// Percent-encodes everything except unreserved characters.
fn encode(text: &str) -> String {
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

/// Trims the text and cuts it to the query limit on a character boundary.
pub fn query(text: &str) -> Option<&str> {
	let text = text.trim();
	if text.is_empty() {
		return None;
	}
	let mut end = text.len().min(MAX_QUERY);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	Some(&text[..end])
}

/// Builds an address with `make`, shortening the text until the address fits.
fn fitted(text: &str, make: impl Fn(&str) -> String) -> String {
	let mut end = text.len();
	loop {
		let url = make(&text[..end]);
		if url.len() <= MAX_QUERY || end == 0 {
			return url;
		}
		end = end.saturating_sub((url.len() - MAX_QUERY).div_ceil(3).max(1));
		while !text.is_char_boundary(end) {
			end -= 1;
		}
	}
}

pub fn translate_url(preferences: &Preferences, text: &str) -> String {
	let language = preferences.target_language.as_str();
	if preferences.provider == "deepl" {
		fitted(text, |text| {
			format!(
				"https://www.deepl.com/translator#auto/{language}/{}",
				encode(text)
			)
		})
	} else {
		fitted(text, |text| {
			format!(
				"https://translate.google.com/?sl=auto&tl={language}&text={}&op=translate",
				encode(text)
			)
		})
	}
}

pub fn search_url(preferences: &Preferences, text: &str) -> String {
	let prefix = match preferences.search_engine.as_str() {
		"google" => "https://www.google.com/search?q=",
		"brave" => "https://search.brave.com/search?q=",
		"bing" => "https://www.bing.com/search?q=",
		"wikipedia" => "https://en.wikipedia.org/w/index.php?search=",
		_ => "https://duckduckgo.com/?q=",
	};
	fitted(text, |text| format!("{prefix}{}", encode(text)))
}

fn open(url: String) -> ExtensionOutput {
	ExtensionOutput::effect(HostEffect::AppAction {
		action: AppAction::OpenExternalLink { url },
	})
}

fn label(list: &[(&str, &str)], id: &str) -> String {
	list.iter()
		.find(|(value, _)| *value == id)
		.map_or(id, |(_, label)| label)
		.to_owned()
}

fn from_label(list: &[(&str, &str)], label: Option<&str>) -> Option<String> {
	list.iter()
		.find(|(_, name)| Some(*name) == label)
		.map(|(id, _)| (*id).to_owned())
}

fn settings_panel(preferences: &Preferences, note: Option<&str>) -> Vec<Element> {
	let select = |id: &str, title: &str, list: &[(&str, &str)], value: &str| Element::Select {
		id: id.into(),
		label: title.into(),
		options: list.iter().map(|(_, name)| (*name).to_owned()).collect(),
		value: label(list, value),
	};
	let mut panel = vec![Element::Heading {
		text: "External Search Tools".into(),
	}];
	if let Some(note) = note {
		panel.push(Element::Text { text: note.into() });
	}
	panel.extend([
		select("provider", "Translator", PROVIDERS, &preferences.provider),
		select(
			"language",
			"Translate to",
			LANGUAGES,
			&preferences.target_language,
		),
		select("engine", "Search with", ENGINES, &preferences.search_engine),
		Element::Button {
			id: "save".into(),
			label: "Save".into(),
		},
	]);
	panel
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let base = &input.invocation.invocation.invocation;
	let preferences = Preferences::load(base.storage.as_deref());
	match input.action() {
		"translate" | "translate-message" | "search" => {
			let Some(text) = query(input.selected_message().unwrap_or_default()) else {
				return ExtensionOutput::panel(vec![Element::Text {
					text: "This message has no text.".into(),
				}]);
			};
			if input.action() == "search" {
				open(search_url(&preferences, text))
			} else {
				open(translate_url(&preferences, text))
			}
		}
		_ if input.button() == Some("save") => {
			let chosen = Preferences {
				provider: from_label(PROVIDERS, input.value("provider"))
					.unwrap_or(preferences.provider.clone()),
				target_language: from_label(LANGUAGES, input.value("language"))
					.unwrap_or(preferences.target_language.clone()),
				search_engine: from_label(ENGINES, input.value("engine"))
					.unwrap_or(preferences.search_engine.clone()),
			};
			let mut output = ExtensionOutput::panel(settings_panel(&chosen, Some("Saved.")));
			if output.output.output.set_storage_json(&chosen).is_err() {
				output = ExtensionOutput::panel(settings_panel(
					&preferences,
					Some("Could not save these preferences."),
				));
			}
			output
		}
		_ => ExtensionOutput::panel(settings_panel(&preferences, None)),
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::testing::TestHost;

	#[test]
	fn earlier_saved_preferences_load_and_invalid_ones_fall_back() {
		let saved = r#"{"provider":"deepl","target_language":"ja","search_engine":"brave"}"#;
		let loaded = Preferences::load(Some(saved));
		assert_eq!(
			(loaded.provider.as_str(), loaded.search_engine.as_str()),
			("deepl", "brave")
		);
		for broken in [r#"{"provider":"evil"}"#, r#"{"other":1}"#, "not json"] {
			assert_eq!(Preferences::load(Some(broken)), Preferences::default());
		}
	}

	#[test]
	fn addresses_encode_the_text_and_stay_within_the_limit() {
		let preferences = Preferences::default();
		assert_eq!(
			search_url(&preferences, "a&b #c"),
			"https://duckduckgo.com/?q=a%26b%20%23c"
		);
		let long = "ж".repeat(2000);
		for url in [
			translate_url(&preferences, query(&long).unwrap()),
			search_url(&preferences, query(&long).unwrap()),
		] {
			assert!(url.len() <= MAX_QUERY, "{}", url.len());
		}
		assert!(query("   ").is_none());
	}

	#[test]
	fn the_save_button_stores_the_chosen_values() {
		let mut input = ExtensionInvocation::default();
		let base = &mut input.invocation.invocation.invocation;
		base.action = "open".into();
		base.values.insert("provider".into(), "DeepL".into());
		base.values.insert("language".into(), "Japanese".into());
		base.values.insert("engine".into(), "Wikipedia".into());
		input.button = Some("save".into());
		let output = TestHost::default().run(input, handle).unwrap();
		assert_eq!(
			Preferences::load(output.output.output.storage.as_deref()),
			Preferences {
				provider: "deepl".into(),
				target_language: "ja".into(),
				search_engine: "wikipedia".into(),
			}
		);
	}

	#[test]
	fn the_toolbar_button_opens_the_translator_with_saved_choices() {
		let mut input = ExtensionInvocation::default();
		let base = &mut input.invocation.invocation.invocation;
		base.action = "translate-message".into();
		base.selected_message = Some(" hola ".into());
		base.storage =
			Some(r#"{"provider":"deepl","target_language":"en","search_engine":"bing"}"#.into());
		let output = TestHost::default().run(input, handle).unwrap();
		assert_eq!(
			output.output.effects,
			[HostEffect::AppAction {
				action: AppAction::OpenExternalLink {
					url: "https://www.deepl.com/translator#auto/en/hola".into()
				}
			}]
		);
	}
}
