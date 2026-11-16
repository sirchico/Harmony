//! Translate: translate messages with Google Translate or DeepL.
//!
//! - A Translate button in the message hover toolbar and a Translate item in the message menu
//!   show the translation under the message: "(translated from Spanish - Dismiss)".
//! - A translate button beside the composer opens the Translate window: the languages incoming
//!   and your own messages are translated from and to, and Auto Translate. Shift-click or
//!   right-click the button to toggle Auto Translate; it turns green while on.
//! - With Auto Translate on, your messages are translated before they are sent.
//!
//! DeepL uses your API key, kept as a secret that Harmony adds to the request. Written for
//! Harmony using only the public API v2 SDK.
use harmony_extension_sdk::{
	ContributionIcon, ContributionItem, ContributionOp, ContributionTarget, Element,
	ExtensionInvocation, ExtensionOutput, HostRequest, HttpRequest, ResponseValue, Slot,
	serde_json,
};
use serde::{Deserialize, Serialize};

/// Longest message text sent for translation, in bytes.
const MAX_TEXT: usize = 4000;
/// How long a translation stays under its message.
const SHOWN_MS: u64 = 6 * 60 * 60 * 1000;
/// The composer button's color while Auto Translate is on.
const AUTO_ON: &str = "#23A55A";

/// Google's language codes and names; "auto" detects the language.
pub const LANGUAGES: &[(&str, &str)] = &[
	("ar", "Arabic"),
	("bg", "Bulgarian"),
	("zh-CN", "Chinese (Simplified)"),
	("zh-TW", "Chinese (Traditional)"),
	("cs", "Czech"),
	("da", "Danish"),
	("nl", "Dutch"),
	("en", "English"),
	("fi", "Finnish"),
	("fr", "French"),
	("de", "German"),
	("el", "Greek"),
	("iw", "Hebrew"),
	("hi", "Hindi"),
	("hu", "Hungarian"),
	("id", "Indonesian"),
	("it", "Italian"),
	("ja", "Japanese"),
	("ko", "Korean"),
	("no", "Norwegian"),
	("fa", "Persian"),
	("pl", "Polish"),
	("pt", "Portuguese"),
	("ro", "Romanian"),
	("ru", "Russian"),
	("es", "Spanish"),
	("sv", "Swedish"),
	("th", "Thai"),
	("tr", "Turkish"),
	("uk", "Ukrainian"),
	("vi", "Vietnamese"),
];
const DETECT: &str = "Detect language";

/// Languages and Auto Translate, edited in the Translate window and kept in local storage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
	pub received_input: String,
	pub received_output: String,
	pub sent_input: String,
	pub sent_output: String,
	pub auto_translate: bool,
	pub dismissed_auto_alert: bool,
}
impl Default for Prefs {
	fn default() -> Self {
		Self {
			received_input: "auto".into(),
			received_output: "en".into(),
			sent_input: "auto".into(),
			sent_output: "en".into(),
			auto_translate: false,
			dismissed_auto_alert: false,
		}
	}
}

fn prefs(input: &ExtensionInvocation) -> Prefs {
	input
		.invocation
		.invocation
		.invocation
		.storage_json::<Prefs>()
		.ok()
		.flatten()
		.unwrap_or_default()
}

fn save(output: &mut ExtensionOutput, prefs: &Prefs) {
	let _ = output.output.output.set_storage_json(prefs);
}

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

pub fn language_name(code: &str) -> String {
	if code == "auto" {
		return DETECT.into();
	}
	let lower = code.to_ascii_lowercase();
	LANGUAGES
		.iter()
		.find(|(id, _)| id.eq_ignore_ascii_case(code))
		.or_else(|| {
			// DeepL answers with bare upper-case codes such as "ES" or "ZH".
			let base = lower.split(['-', '_']).next().unwrap_or(&lower);
			let base = if base == "he" { "iw" } else { base };
			LANGUAGES
				.iter()
				.find(|(id, _)| id.split('-').next() == Some(base))
		})
		.map_or_else(|| code.to_owned(), |(_, name)| (*name).to_owned())
}

fn language_code(name: &str) -> Option<&'static str> {
	if name == DETECT {
		return Some("auto");
	}
	LANGUAGES
		.iter()
		.find(|(_, label)| *label == name)
		.map(|(code, _)| *code)
}

/// DeepL's code for a Google one: upper case, its own Chinese, Hebrew and Norwegian codes,
/// and a regional variant where DeepL needs one as a target.
fn deepl_code(code: &str, target: bool) -> String {
	match (code, target) {
		("zh-CN", _) => "ZH-HANS".into(),
		("zh-TW", _) => "ZH-HANT".into(),
		("iw", _) => "HE".into(),
		("no", _) => "NB".into(),
		("en", true) => "EN-US".into(),
		("pt", true) => "PT-BR".into(),
		_ => code.to_ascii_uppercase(),
	}
}

/// The request for one translation from `from` ("auto" detects) into `to`.
pub fn request(service: &str, from: &str, to: &str, text: &str) -> HttpRequest {
	match service {
		"deepl" | "deepl-free" | "deepl-pro" => {
			let host = if service == "deepl-pro" {
				"api.deepl.com"
			} else {
				"api-free.deepl.com"
			};
			let mut body =
				serde_json::json!({ "text": [text], "target_lang": deepl_code(to, true) });
			if from != "auto" {
				body["source_lang"] = deepl_code(from, false).into();
			}
			HttpRequest::post(format!("https://{host}/v2/translate"), body.to_string())
				.header("Content-Type", "application/json")
				.secret_header("Authorization", "deepl-key", "DeepL-Auth-Key ")
		}
		_ => HttpRequest::get(format!(
			"https://translate.googleapis.com/translate_a/single?client=gtx&sl={from}&tl={to}&dt=t&dj=1&source=input&q={}",
			encode(text)
		)),
	}
}

#[derive(Deserialize)]
struct Google {
	#[serde(default)]
	sentences: Vec<GoogleSentence>,
	#[serde(default)]
	src: Option<String>,
}
#[derive(Deserialize)]
struct GoogleSentence {
	#[serde(default)]
	trans: Option<String>,
}
#[derive(Deserialize)]
struct DeepL {
	translations: Vec<DeepLText>,
}
#[derive(Deserialize)]
struct DeepLText {
	#[serde(default)]
	detected_source_language: Option<String>,
	text: String,
}

/// (detected language, translation) from a service's answer.
pub fn parse(service: &str, body: &str) -> Option<(String, String)> {
	if service.starts_with("deepl") {
		let answer: DeepL = serde_json::from_str(body).ok()?;
		let first = answer.translations.into_iter().next()?;
		Some((
			first.detected_source_language.unwrap_or_default(),
			first.text,
		))
	} else {
		let answer: Google = serde_json::from_str(body).ok()?;
		let text: String = answer
			.sentences
			.into_iter()
			.filter_map(|sentence| sentence.trans)
			.collect();
		(!text.is_empty()).then(|| (answer.src.unwrap_or_default(), text))
	}
}

fn clip(text: &str) -> &str {
	let mut end = text.len().min(MAX_TEXT);
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	&text[..end]
}

/// The service's translation from the previous round, or why there is none.
fn translation(input: &ExtensionInvocation, service: &str) -> Result<(String, String), String> {
	let Some(response) = input.response("translation") else {
		return Err("The translation service did not answer.".into());
	};
	match (&response.value, &response.error) {
		(Some(ResponseValue::Http { response }), _) if response.status == 200 => {
			parse(service, &response.body).ok_or_else(|| {
				"The translation service gave an answer this extension could not read.".into()
			})
		}
		(Some(ResponseValue::Http { response }), _) if matches!(response.status, 401 | 403) => {
			Err("The translation service refused the API key.".into())
		}
		(Some(ResponseValue::Http { response }), _)
			if response.status == 429 || response.status == 456 =>
		{
			Err("The translation service's limit was reached; try again later.".into())
		}
		(_, Some(error)) => Err(format!("Could not translate: {}", error.message)),
		_ => Err("The translation service did not answer.".into()),
	}
}

fn problem(text: impl Into<String>) -> ExtensionOutput {
	ExtensionOutput::panel(vec![Element::Text { text: text.into() }])
}

fn service(input: &ExtensionInvocation) -> String {
	match input.setting_text("service").unwrap_or("google") {
		"deepl-free" => "deepl".into(),
		other => other.into(),
	}
}

fn missing_key(input: &ExtensionInvocation, service: &str) -> bool {
	service.starts_with("deepl") && !input.secret_set("deepl-key")
}

fn translate_request(text: &str, service: &str, from: &str, to: &str) -> ExtensionOutput {
	let mut output = ExtensionOutput::default();
	output.request(HostRequest::Http {
		id: "translation".into(),
		request: Box::new(request(service, from, to, clip(text))),
	});
	output
}

/// The hover-toolbar button and the message menu item: the translation under the message.
fn translate_message(input: &ExtensionInvocation) -> ExtensionOutput {
	let service = service(input);
	let Some(message) = input.selected_message_ref.clone() else {
		return problem("Use Translate on a message.");
	};
	let text = input.selected_message().unwrap_or_default().trim();
	if text.is_empty() {
		return problem("This message has no text to translate.");
	}
	if missing_key(input, &service) {
		return problem("Add your DeepL API key in Translate's settings, or switch to Google.");
	}
	let prefs = prefs(input);
	if input.response("translation").is_none() {
		return translate_request(
			text,
			&service,
			&prefs.received_input,
			&prefs.received_output,
		);
	}
	let (from, translated) = match translation(input, &service) {
		Ok(answer) => answer,
		Err(error) => return problem(error),
	};
	let mut output = ExtensionOutput::default();
	let from = if from.is_empty() {
		language_name(&prefs.received_input)
	} else {
		language_name(&from)
	};
	let shown: String = translated.chars().take(2000).collect();
	output.contribute(ContributionOp::Set {
		slot: Slot::MessageAccessory,
		key: format!("tr-{}", message.message_id),
		target: ContributionTarget::message(message.message_id.clone()),
		item: ContributionItem::text(shown)
			.icon(ContributionIcon::Translate)
			.italic()
			.note(format!("translated from {from}"))
			.dismissible(),
		ttl_ms: Some(SHOWN_MS),
	});
	output
}

/// The translate button beside the composer, green while Auto Translate is on.
fn composer_button(prefs: &Prefs) -> ContributionOp {
	let mut item = ContributionItem::text("Translate")
		.tooltip("Open Translate Modal")
		.icon(ContributionIcon::Translate)
		.action("open")
		.secondary_action("toggle-auto");
	if prefs.auto_translate {
		item = item.color(AUTO_ON);
	}
	ContributionOp::set(
		Slot::ComposerButton,
		"translate",
		ContributionTarget::App,
		item,
	)
}

fn select(id: &str, label: &str, value: &str, detect: bool) -> Element {
	let mut options: Vec<String> = Vec::with_capacity(LANGUAGES.len() + 1);
	if detect {
		options.push(DETECT.into());
	}
	options.extend(LANGUAGES.iter().map(|(_, name)| (*name).to_owned()));
	Element::Select {
		id: id.into(),
		label: label.into(),
		options,
		value: language_name(value),
	}
}

/// The Translate window: languages for both directions and Auto Translate.
fn window(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut prefs = prefs(input);
	let mut output = ExtensionOutput::default();
	if input.button() == Some("save") {
		let pick = |id: &str, current: &str| {
			input
				.value(id)
				.and_then(language_code)
				.unwrap_or(current)
				.to_owned()
		};
		prefs.received_input = pick("received-input", &prefs.received_input);
		prefs.received_output = pick("received-output", &prefs.received_output);
		prefs.sent_input = pick("sent-input", &prefs.sent_input);
		prefs.sent_output = pick("sent-output", &prefs.sent_output);
		// Translating into "detect" is not a thing; keep a real target.
		if prefs.received_output == "auto" {
			prefs.received_output = "en".into();
		}
		if prefs.sent_output == "auto" {
			prefs.sent_output = "en".into();
		}
		prefs.auto_translate = input.value("auto-translate") == Some("true");
		save(&mut output, &prefs);
		output.contribute(composer_button(&prefs));
		return output;
	}
	output.output.output.panel = vec![
		Element::Heading {
			text: "Translate".into(),
		},
		select(
			"received-input",
			"Language incoming messages are translated from",
			&prefs.received_input,
			true,
		),
		select(
			"received-output",
			"Language incoming messages are translated to",
			&prefs.received_output,
			false,
		),
		select(
			"sent-input",
			"Language your messages are translated from",
			&prefs.sent_input,
			true,
		),
		select(
			"sent-output",
			"Language your messages are translated to",
			&prefs.sent_output,
			false,
		),
		Element::Separator,
		Element::Checkbox {
			id: "auto-translate".into(),
			label: "Auto Translate".into(),
			checked: prefs.auto_translate,
		},
		Element::Text {
			text: "Automatically translate your messages before sending. You can also Shift+click or right-click the translate button to toggle this".into(),
		},
		Element::Button {
			id: "save".into(),
			label: "Save".into(),
		},
	];
	output
}

/// Shift-click or right-click on the composer button: turns Auto Translate on or off. The first
/// time it is turned on, a notice explains it.
fn toggle_auto(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut prefs = prefs(input);
	let mut output = ExtensionOutput::default();
	match input.button() {
		Some("got-it") => {
			prefs.dismissed_auto_alert = input.value("dont-show") == Some("true");
			save(&mut output, &prefs);
			return output;
		}
		Some("disable") => prefs.auto_translate = false,
		_ => prefs.auto_translate = !prefs.auto_translate,
	}
	save(&mut output, &prefs);
	output.contribute(composer_button(&prefs));
	if prefs.auto_translate && !prefs.dismissed_auto_alert && input.button().is_none() {
		output.output.output.panel = vec![
			Element::Heading {
				text: "Auto-Translate Enabled".into(),
			},
			Element::Text {
				text: "You just enabled Auto Translate! Any message will automatically be translated before being sent.".into(),
			},
			Element::Checkbox {
				id: "dont-show".into(),
				label: "Don't show again".into(),
				checked: false,
			},
			Element::Row {
				children: vec![
					Element::Button {
						id: "disable".into(),
						label: "Disable Auto-Translate".into(),
					},
					Element::Button {
						id: "got-it".into(),
						label: "Got it".into(),
					},
				],
			},
		];
	}
	output
}

/// With Auto Translate on, your message is translated before it is sent. If the translation
/// fails or is slow, Harmony sends what you typed.
fn before_send(input: &ExtensionInvocation) -> ExtensionOutput {
	let prefs = prefs(input);
	let service = service(input);
	let text = input
		.invocation
		.invocation
		.invocation
		.composer
		.clone()
		.unwrap_or_default();
	if !prefs.auto_translate || text.trim().is_empty() || missing_key(input, &service) {
		return ExtensionOutput::default();
	}
	if input.response("translation").is_none() {
		return translate_request(&text, &service, &prefs.sent_input, &prefs.sent_output);
	}
	let mut output = ExtensionOutput::default();
	if let Ok((_, translated)) = translation(input, &service)
		&& !translated.trim().is_empty()
	{
		output.output.output.replacement = Some(translated.chars().take(4000).collect());
	}
	output
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	match input.action() {
		"translate" | "translate-menu" => translate_message(&input),
		"open" => window(&input),
		"toggle-auto" => toggle_auto(&input),
		"auto" => before_send(&input),
		// Activation: draw the composer button in its current state.
		_ => {
			let mut output = ExtensionOutput::default();
			output.contribute(composer_button(&prefs(&input)));
			output
		}
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{SelectedMessageRef, SettingValue, testing::TestHost};

	fn base(action: &str, service: &str, prefs: &Prefs) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = action.into();
		input.invocation.invocation.invocation.storage =
			Some(serde_json::to_string(prefs).unwrap());
		input
			.settings
			.insert("service".into(), SettingValue::Text(service.into()));
		input
	}

	fn on_message(service: &str, key_set: bool) -> ExtensionInvocation {
		let mut input = base("translate", service, &Prefs::default());
		input.invocation.invocation.invocation.selected_message = Some("hola amigos".into());
		input.selected_message_ref = Some(SelectedMessageRef {
			channel_id: "2".into(),
			message_id: "3".into(),
			author_id: "4".into(),
			guild_id: None,
		});
		input.secrets.insert("deepl-key".into(), key_set);
		input
	}

	#[test]
	fn translation_appears_under_the_message_with_its_language_and_dismiss() {
		let mut host = TestHost::default();
		host.web.insert(
			request("google", "auto", "en", "hola amigos").url,
			(
				200,
				r#"{"sentences":[{"trans":"hello "},{"trans":"friends"}],"src":"es"}"#.into(),
			),
		);
		let output = host.run(on_message("google", false), handle).unwrap();
		assert!(output.output.output.panel.is_empty());
		let (_, item) = &host.contributions[&(Slot::MessageAccessory, "tr-3".into())];
		assert_eq!(item.text, "hello friends");
		assert_eq!(item.note.as_deref(), Some("translated from Spanish"));
		assert!(item.dismissible && item.italic);
	}

	#[test]
	fn deepl_sends_the_key_as_a_secret_header_and_uses_its_own_codes() {
		let deepl = request("deepl", "zh-TW", "en", "hi");
		assert_eq!(deepl.url, "https://api-free.deepl.com/v2/translate");
		assert_eq!(deepl.secret_headers[0].secret, "deepl-key");
		let body = deepl.body.unwrap();
		assert!(body.contains("\"EN-US\"") && body.contains("\"ZH-HANT\""));
		assert!(
			!deepl
				.headers
				.iter()
				.any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
		);
		let mut host = TestHost::default();
		let missing = host.run(on_message("deepl-free", false), handle).unwrap();
		assert!(host.requests.is_empty());
		assert!(!missing.output.output.panel.is_empty());
		assert_eq!(
			parse(
				"deepl",
				r#"{"translations":[{"detected_source_language":"ES","text":"Hi"}]}"#
			),
			Some(("ES".into(), "Hi".into()))
		);
		assert_eq!(language_name("ES"), "Spanish");
	}

	/// Auto Translate rewrites only while on, with the "sent" languages; the window saves
	/// language names back as codes and repaints the button green.
	#[test]
	fn auto_translate_uses_sent_languages_and_the_window_saves_choices() {
		let prefs = Prefs {
			sent_output: "de".into(),
			auto_translate: true,
			..Prefs::default()
		};
		let mut send = base("auto", "google", &prefs);
		send.invocation.invocation.invocation.composer = Some("good morning".into());
		let mut host = TestHost::default();
		host.web.insert(
			request("google", "auto", "de", "good morning").url,
			(
				200,
				r#"{"sentences":[{"trans":"guten Morgen"}],"src":"en"}"#.into(),
			),
		);
		let output = host.run(send.clone(), handle).unwrap();
		assert_eq!(
			output.output.output.replacement.as_deref(),
			Some("guten Morgen")
		);

		let off = base("auto", "google", &Prefs::default());
		let mut host = TestHost::default();
		assert!(
			host.run(off, handle)
				.unwrap()
				.output
				.output
				.replacement
				.is_none()
		);
		assert!(host.requests.is_empty());

		let mut save = base("open", "google", &Prefs::default());
		save.button = Some("save".into());
		let values = &mut save.invocation.invocation.invocation.values;
		values.insert("received-output".into(), "Japanese".into());
		values.insert("sent-output".into(), DETECT.into());
		values.insert("auto-translate".into(), "true".into());
		let mut host = TestHost::default();
		let output = host.run(save, handle).unwrap();
		let saved: Prefs =
			serde_json::from_str(output.output.output.storage.as_deref().unwrap()).unwrap();
		assert_eq!(saved.received_output, "ja");
		assert_eq!(saved.sent_output, "en");
		assert!(saved.auto_translate);
		let (_, button) = &host.contributions[&(Slot::ComposerButton, "translate".into())];
		assert_eq!(button.color.as_deref(), Some(AUTO_ON));
	}
}
