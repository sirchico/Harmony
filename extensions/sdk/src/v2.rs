//! API v2 for extension authors: declarative settings, key-value storage, host requests with
//! continuation, media links, attachment reads and server lookups.
//!
//! Declare `"api_version": 2` in the manifest to use these. Everything is additive: the
//! Wasm exports, `export!` and every v1 type keep working, and a v2 extension can still use
//! all v1 inputs and outputs through [`ExtensionInvocation::invocation`] and
//! [`ExtensionOutput::output`].
//!
//! ```
//! use harmony_extension_sdk::{ExtensionInvocation, ExtensionOutput, HostRequest, ResponseValue};
//! fn handle(input: ExtensionInvocation) -> ExtensionOutput {
//!     let mut output = ExtensionOutput::default();
//!     match input.response("visits").and_then(|response| response.value.as_ref()) {
//!         None => output.request(HostRequest::KvGet { id: "visits".into(), keys: vec!["count".into()] }),
//!         Some(ResponseValue::Kv { values }) => {
//!             let count: u64 = values.get("count").cloned().flatten()
//!                 .and_then(|text| text.parse().ok()).unwrap_or(0) + 1;
//!             output.kv_set("count", count.to_string());
//!             output.output.output.replacement = Some(format!("Visit {count}"));
//!         }
//!         Some(_) => {}
//!     }
//!     output
//! }
//! # let _ = handle;
//! ```
use crate::{
	AppOutput, Element, ExtendedAppInvocation, HostEffect, MenuTarget, SelectedMessageRef,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Manifest level that enables the types in this module.
pub const API_V2: u32 = 2;
/// Requests one round may return; the host rejects more.
pub const MAX_REQUESTS_PER_ROUND: usize = 8;
/// Continuation rounds one action may take before the host stops it.
pub const MAX_REQUEST_ROUNDS: u8 = 4;
/// Size of the opaque `continuation` text, in bytes.
pub const MAX_CONTINUATION_BYTES: usize = 8 * 1024;
/// Largest attachment read in one response, in bytes.
pub const MAX_ATTACHMENT_READ_BYTES: u32 = 160 * 1024;

/// A setting value: `toggle` is `Bool`, `number` is `Number`, `text` and `choice` are `Text`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
	Bool(bool),
	Number(i64),
	Text(String),
}

/// Every v2 input. Fields the host does not send keep their defaults, so the same handler
/// runs for message, menu, panel and event actions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionInvocation {
	/// Action ID, panel values, v1 storage, host info, app data and events.
	#[serde(flatten)]
	pub invocation: ExtendedAppInvocation,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub selected_message_ref: Option<SelectedMessageRef>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub menu_target: Option<MenuTarget>,
	/// Current values of the declared settings; defaults are filled in by the host.
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub settings: BTreeMap<String, SettingValue>,
	/// Whether each declared secret has been set. The values themselves are never sent.
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub secrets: BTreeMap<String, bool>,
	/// Media on the message a `message` action was chosen on (`media_links`).
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub media: Vec<MediaLink>,
	/// Answers to the previous round's requests.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub responses: Vec<HostResponse>,
	/// What the previous round returned in `continuation`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub continuation: Option<String>,
	/// 0 on the first call of an action, then 1, 2, ... for each continuation.
	#[serde(default)]
	pub round: u8,
	/// The message event of a `message_event` action, with the names and mention flag API v2
	/// adds. (It is read from the same input as the v1 event, which is then left empty.)
	#[serde(
		default,
		rename = "message_event",
		skip_serializing_if = "Option::is_none"
	)]
	pub message: Option<MessageEventInfo>,
	/// The panel button that re-ran this action. Buttons whose IDs are not declared actions
	/// keep the action and its target (message, menu item) and arrive here instead.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub button: Option<String>,
}

impl ExtensionInvocation {
	/// The ID of the action that ran.
	pub fn action(&self) -> &str {
		&self.invocation.invocation.invocation.action
	}
	/// A panel value, as in [`crate::Invocation::value`].
	pub fn value(&self, id: &str) -> Option<&str> {
		self.invocation.invocation.invocation.value(id)
	}
	/// The selected message's text for a `message` action.
	pub fn selected_message(&self) -> Option<&str> {
		self.invocation
			.invocation
			.invocation
			.selected_message
			.as_deref()
	}
	pub fn host(&self) -> Option<&crate::HostInfo> {
		self.invocation.invocation.host.as_ref()
	}
	pub fn setting(&self, id: &str) -> Option<&SettingValue> {
		self.settings.get(id)
	}
	/// A `toggle` setting; `false` when absent.
	pub fn setting_bool(&self, id: &str) -> bool {
		matches!(self.settings.get(id), Some(SettingValue::Bool(true)))
	}
	/// A `text` or `choice` setting.
	pub fn setting_text(&self, id: &str) -> Option<&str> {
		match self.settings.get(id) {
			Some(SettingValue::Text(text)) => Some(text),
			_ => None,
		}
	}
	/// A `number` setting.
	pub fn setting_number(&self, id: &str) -> Option<i64> {
		match self.settings.get(id) {
			Some(SettingValue::Number(number)) => Some(*number),
			_ => None,
		}
	}
	/// Whether the user has stored the named secret.
	pub fn secret_set(&self, id: &str) -> bool {
		self.secrets.get(id).copied().unwrap_or(false)
	}
	/// The panel button that re-ran this action, if any.
	pub fn button(&self) -> Option<&str> {
		self.button.as_deref()
	}
	/// The answer to the request with this ID from the previous round.
	pub fn response(&self, id: &str) -> Option<&HostResponse> {
		self.responses.iter().find(|response| response.id == id)
	}
}

/// Every v2 output: a v1 [`crate::Output`] with host effects, plus storage changes and
/// requests. A round that returns `requests` may only carry `kv` and `continuation`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionOutput {
	#[serde(flatten)]
	pub output: AppOutput,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub kv: Vec<KvOp>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub requests: Vec<HostRequest>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub continuation: Option<String>,
	/// Changes to the items Harmony draws for you; see [`crate::ContributionOp`].
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub contributions: Vec<crate::ContributionOp>,
	/// System notifications to show (event actions with `notifications`).
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub notifications: Vec<Notification>,
	/// Lines to append to this extension's log (needs `storage` and a log quota).
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub log: Vec<String>,
	/// Empties the log before `log` lines are appended.
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub log_clear: bool,
	/// GIF picker results (`gif_search` actions only), at most [`MAX_GIF_RESULTS`].
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub gifs: Vec<GifResult>,
	/// Categories for the GIF picker's home page (`gif_search` with an empty query), at most
	/// [`MAX_GIF_CATEGORIES`].
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub gif_categories: Vec<GifCategoryResult>,
}

/// Most categories one home page may return.
pub const MAX_GIF_CATEGORIES: usize = 32;

/// A GIF picker home category: choosing it searches for `name`; `preview` is shown behind it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GifCategoryResult {
	pub name: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub preview: Option<String>,
}

/// Most GIFs one search may return.
pub const MAX_GIF_RESULTS: usize = 50;

/// One GIF for the picker. `url` is sent when it is chosen; `preview` is shown in the picker
/// and must be a Tenor, Klipy or Giphy media address without a query string.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GifResult {
	pub id: String,
	#[serde(default)]
	pub title: String,
	pub url: String,
	pub preview: String,
	pub width: u32,
	pub height: u32,
}

/// A message event as API v2 delivers it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEventInfo {
	pub kind: crate::MessageEventKind,
	pub channel_id: String,
	pub message_id: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub author_id: Option<String>,
	/// The text, for created messages and edits that changed it. Deletes never carry text.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub content: Option<String>,
	/// The author's name as Harmony shows it (created and edited messages).
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub author_name: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub guild_id: Option<String>,
	/// Where the message is, such as `Server · #channel`, or a conversation's name.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub place: Option<String>,
	/// Whether it mentions the current account directly or with @everyone.
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub mentions_me: bool,
	/// Whether its conversation is the one open on screen.
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub open: bool,
}

/// A system notification. Harmony shows a few per minute per extension, prefixed with the
/// extension's name, and none while notifications are off or you are in Do Not Disturb.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
	pub title: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub body: String,
	/// Clicking the notification opens this conversation.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub channel_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub message_id: Option<String>,
}

/// One line of the extension's log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
	pub seq: u64,
	pub at_ms: u64,
	pub text: String,
}

/// Outgoing text a `before_send` hook may return, in bytes.
pub const MAX_SEND_TEXT: usize = 16 * 1024;
/// How long Harmony waits for all `before_send` hooks together, in milliseconds.
pub const SEND_HOOK_BUDGET_MS: u64 = 1500;
/// The hold when your extension has `network`: a hook may then make `http` requests.
pub const SEND_HOOK_NETWORK_BUDGET_MS: u64 = 5000;
/// Most lines one `log_read` returns.
pub const MAX_LOG_READ: u32 = 200;

impl ExtensionOutput {
	/// Adds, replaces or removes one interface item.
	pub fn contribute(&mut self, op: crate::ContributionOp) {
		self.contributions.push(op);
	}
	/// A response that only shows a panel.
	pub fn panel(elements: Vec<Element>) -> Self {
		let mut output = Self::default();
		output.output.output.panel = elements;
		output
	}
	/// A response that proposes one host effect, such as copying text.
	pub fn effect(effect: HostEffect) -> Self {
		let mut output = Self::default();
		output.output.effects.push(effect);
		output
	}
	/// Asks the host for something; the answer arrives in the next round's `responses`.
	pub fn request(&mut self, request: HostRequest) {
		self.requests.push(request);
	}
	pub fn kv_set(&mut self, key: impl Into<String>, value: impl Into<String>) {
		self.kv.push(KvOp::Set {
			key: key.into(),
			value: value.into(),
		});
	}
	pub fn kv_delete(&mut self, key: impl Into<String>) {
		self.kv.push(KvOp::Delete { key: key.into() });
	}
}

/// One change to the extension's key-value store. Needs `storage`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum KvOp {
	Set { key: String, value: String },
	Delete { key: String },
	Clear,
}

/// Something to fetch before the extension runs again for the same action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostRequest {
	/// Values of the named keys; missing keys are `None`. Needs `storage`.
	KvGet { id: String, keys: Vec<String> },
	/// Keys starting with `prefix`, sorted, at most `limit` (256 by default). Needs `storage`.
	KvList {
		id: String,
		#[serde(default)]
		prefix: String,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		limit: Option<u32>,
	},
	/// The start of an attachment on the chosen message. Needs `attachment_contents`.
	AttachmentRead {
		id: String,
		attachment_id: String,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		max_bytes: Option<u32>,
	},
	/// Public details of a server the account is in. Needs `server_profiles`.
	ServerProfile { id: String, guild_id: String },
	/// An HTTPS request Harmony makes to one of the manifest's `origins`. Needs `network`.
	Http {
		id: String,
		request: Box<HttpRequest>,
	},
	/// This extension's log lines, newest first, older than `before` (a `seq`) when given.
	LogRead {
		id: String,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		before: Option<u64>,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		limit: Option<u32>,
	},
}

/// The answer to one request: `value` on success, `error` otherwise.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostResponse {
	pub id: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub value: Option<ResponseValue>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<RequestError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseValue {
	Kv {
		values: BTreeMap<String, Option<String>>,
	},
	KvKeys {
		keys: Vec<String>,
		truncated: bool,
	},
	Attachment {
		content: AttachmentContent,
	},
	ServerProfile {
		profile: Box<ServerProfile>,
	},
	LogEntries {
		entries: Vec<LogEntry>,
		more: bool,
	},
	Http {
		response: Box<HttpResponse>,
	},
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
	Get,
	Post,
	Put,
	Patch,
	Delete,
}

/// A header Harmony fills from one of your `secret` settings; your code never sees the value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretHeader {
	pub header: String,
	pub secret: String,
	/// Text before the secret, such as `"Bearer "`.
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub prefix: String,
}

/// A query parameter Harmony fills from one of your `secret` settings, for services that take
/// their API key in the address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretParam {
	pub param: String,
	pub secret: String,
}

/// An HTTPS request. Only `https://` on port 443 to a listed origin; redirects are returned,
/// not followed; `Authorization`, `Cookie` and similar headers can only come from secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
	pub method: HttpMethod,
	pub url: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub headers: Vec<(String, String)>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub secret_headers: Vec<SecretHeader>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub secret_params: Vec<SecretParam>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub body: Option<String>,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub body_base64: bool,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub max_bytes: Option<u32>,
}

impl HttpRequest {
	pub fn get(url: impl Into<String>) -> Self {
		Self {
			method: HttpMethod::Get,
			url: url.into(),
			headers: Vec::new(),
			secret_headers: Vec::new(),
			secret_params: Vec::new(),
			body: None,
			body_base64: false,
			max_bytes: None,
		}
	}
	pub fn post(url: impl Into<String>, body: impl Into<String>) -> Self {
		Self {
			method: HttpMethod::Post,
			body: Some(body.into()),
			..Self::get(url)
		}
	}
	pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
		self.headers.push((name.into(), value.into()));
		self
	}
	/// Adds `header: prefix + <secret value>`, filled in by Harmony.
	pub fn secret_header(
		mut self,
		header: impl Into<String>,
		secret: impl Into<String>,
		prefix: impl Into<String>,
	) -> Self {
		self.secret_headers.push(SecretHeader {
			header: header.into(),
			secret: secret.into(),
			prefix: prefix.into(),
		});
		self
	}
	/// Appends `param=<secret value>` to the address, filled in by Harmony.
	pub fn secret_param(mut self, param: impl Into<String>, secret: impl Into<String>) -> Self {
		self.secret_params.push(SecretParam {
			param: param.into(),
			secret: secret.into(),
		});
		self
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
	pub status: u16,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub content_type: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub location: Option<String>,
	pub encoding: ContentEncoding,
	pub body: String,
	pub truncated: bool,
}

/// Why a request failed. Unknown future codes deserialize as `Other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestErrorCode {
	NotGranted,
	NotFound,
	Limit,
	Unavailable,
	Failed,
	Unsupported,
	Invalid,
	#[serde(other)]
	Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestError {
	pub code: RequestErrorCode,
	pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentEncoding {
	Text,
	Base64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentContent {
	pub attachment_id: String,
	pub filename: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub content_type: Option<String>,
	/// The attachment's full size in bytes.
	pub size: u64,
	/// `Text` when the bytes read are valid UTF-8, otherwise `Base64`.
	pub encoding: ContentEncoding,
	pub data: String,
	/// Whether only the start of the attachment was read.
	pub truncated: bool,
}

/// Public server details fetched from Discord by the host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerProfile {
	pub id: String,
	pub name: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub description: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub icon_url: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub banner_url: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub splash_url: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub owner_id: Option<String>,
	/// Unix milliseconds, derived from the server ID.
	pub created_at_ms: u64,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub verification_level: Option<u8>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub explicit_content_filter: Option<u8>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub nsfw_level: Option<u8>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mfa_level: Option<u8>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub premium_tier: Option<u8>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub premium_subscription_count: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub approximate_member_count: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub approximate_presence_count: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub max_members: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub vanity_url_code: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub preferred_locale: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub features: Vec<String>,
	#[serde(default)]
	pub roles: u32,
	#[serde(default)]
	pub emojis: u32,
	#[serde(default)]
	pub stickers: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSource {
	Attachment,
	Embed,
	#[serde(other)]
	Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
	Image,
	Video,
	Audio,
	File,
	#[serde(other)]
	Other,
}

/// A public HTTPS link to media on the chosen message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaLink {
	pub source: MediaSource,
	pub kind: MediaKind,
	pub url: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub attachment_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub filename: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub content_type: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub size: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub width: Option<u32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub height: Option<u32>,
}

/// A local stand-in for Harmony's request loop, for testing handlers without the app.
///
/// It feeds the handler JSON through the same path as `export!`, applies `kv` changes,
/// answers requests from the fixtures you add, and runs continuation rounds up to the host
/// limit. It does not check capabilities; the host does that when the extension is installed.
pub mod testing {
	use super::*;

	#[derive(Clone, Debug, Default)]
	pub struct TestHost {
		pub kv: BTreeMap<String, String>,
		pub attachments: BTreeMap<String, (String, Option<String>, Vec<u8>)>,
		pub servers: BTreeMap<String, ServerProfile>,
		/// Interface items the handler has set, by (slot, key), as Harmony would keep them.
		pub contributions:
			BTreeMap<(crate::Slot, String), (crate::ContributionTarget, crate::ContributionItem)>,
		/// Every input the handler received, in order.
		pub inputs: Vec<ExtensionInvocation>,
		/// The extension's log, oldest first.
		pub log: Vec<LogEntry>,
		/// Every notification the handler asked for.
		pub notifications: Vec<Notification>,
		/// Answers to `http` requests by exact URL: (status, body). Unknown URLs fail.
		pub web: BTreeMap<String, (u16, String)>,
		/// Every `http` request the handler made, in order.
		pub requests: Vec<HttpRequest>,
	}

	impl TestHost {
		/// Adds an attachment the `attachment_read` request can return.
		pub fn attachment(
			&mut self,
			id: &str,
			filename: &str,
			content_type: Option<&str>,
			bytes: &[u8],
		) {
			self.attachments.insert(
				id.into(),
				(
					filename.into(),
					content_type.map(Into::into),
					bytes.to_vec(),
				),
			);
		}

		/// Runs one action to completion, answering requests between rounds, and returns the
		/// final output. Errors name the rule a real host would also enforce.
		pub fn run(
			&mut self,
			mut input: ExtensionInvocation,
			handler: impl Fn(ExtensionInvocation) -> ExtensionOutput,
		) -> Result<ExtensionOutput, String> {
			input.round = 0;
			input.responses.clear();
			input.continuation = None;
			loop {
				self.inputs.push(input.clone());
				let bytes = serde_json::to_vec(&input).map_err(|error| error.to_string())?;
				let output =
					crate::dispatch_typed(&bytes, &handler).map_err(|error| error.to_string())?;
				let output: ExtensionOutput =
					serde_json::from_slice(&output).map_err(|error| error.to_string())?;
				self.apply_kv(&output.kv);
				self.apply_contributions(&output.contributions);
				if output.log_clear {
					self.log.clear();
				}
				for line in &output.log {
					let seq = self.log.last().map_or(1, |entry| entry.seq + 1);
					self.log.push(LogEntry {
						seq,
						at_ms: 0,
						text: line.clone(),
					});
				}
				self.notifications
					.extend(output.notifications.iter().cloned());
				if output.requests.is_empty() {
					return Ok(output);
				}
				if output.requests.len() > MAX_REQUESTS_PER_ROUND {
					return Err("more than 8 requests in one round".into());
				}
				for request in &output.requests {
					// The same limits Harmony enforces, so a test fails where the app would.
					let too_big = match request {
						HostRequest::KvGet { keys, .. } => keys.is_empty() || keys.len() > 64,
						HostRequest::KvList { limit, .. } => {
							limit.is_some_and(|n| n == 0 || n > 256)
						}
						HostRequest::LogRead { limit, .. } => {
							limit.is_some_and(|n| n == 0 || n > MAX_LOG_READ)
						}
						HostRequest::AttachmentRead { max_bytes, .. } => {
							max_bytes.is_some_and(|n| n == 0 || n > MAX_ATTACHMENT_READ_BYTES)
						}
						HostRequest::ServerProfile { .. } => false,
						HostRequest::Http { request, .. } => {
							!request.url.starts_with("https://")
								|| request
									.body
									.as_ref()
									.is_some_and(|body| body.len() > 64 * 1024)
						}
					};
					if too_big {
						return Err(format!("request outside Harmony's limits: {request:?}"));
					}
				}
				if input.round >= MAX_REQUEST_ROUNDS {
					return Err("the action took more than 4 request rounds".into());
				}
				if !output.output.output.panel.is_empty()
					|| !output.output.effects.is_empty()
					|| output.output.output.replacement.is_some()
					|| !output.contributions.is_empty()
				{
					return Err("a round with requests may only return kv and continuation".into());
				}
				for request in &output.requests {
					if let HostRequest::Http { request, .. } = request {
						self.requests.push(request.as_ref().clone());
					}
				}
				input.responses = output
					.requests
					.iter()
					.map(|request| self.answer(request))
					.collect();
				input.continuation = output.continuation;
				input.round += 1;
			}
		}

		fn apply_contributions(&mut self, ops: &[crate::ContributionOp]) {
			for op in ops {
				match op {
					crate::ContributionOp::Set {
						slot,
						key,
						target,
						item,
						..
					} => {
						self.contributions
							.insert((*slot, key.clone()), (target.clone(), item.clone()));
					}
					crate::ContributionOp::Remove { slot, key } => {
						self.contributions.remove(&(*slot, key.clone()));
					}
					crate::ContributionOp::Clear { slot } => self
						.contributions
						.retain(|(item_slot, _), _| slot.is_some_and(|slot| slot != *item_slot)),
				}
			}
		}

		fn apply_kv(&mut self, ops: &[KvOp]) {
			for op in ops {
				match op {
					KvOp::Set { key, value } => {
						self.kv.insert(key.clone(), value.clone());
					}
					KvOp::Delete { key } => {
						self.kv.remove(key);
					}
					KvOp::Clear => self.kv.clear(),
				}
			}
		}

		fn answer(&self, request: &HostRequest) -> HostResponse {
			let ok = |id: &str, value| HostResponse {
				id: id.into(),
				value: Some(value),
				error: None,
			};
			let missing = |id: &str| HostResponse {
				id: id.into(),
				value: None,
				error: Some(RequestError {
					code: RequestErrorCode::NotFound,
					message: "not found".into(),
				}),
			};
			match request {
				HostRequest::KvGet { id, keys } => ok(
					id,
					ResponseValue::Kv {
						values: keys
							.iter()
							.map(|key| (key.clone(), self.kv.get(key).cloned()))
							.collect(),
					},
				),
				HostRequest::KvList { id, prefix, limit } => {
					let limit = limit.unwrap_or(256) as usize;
					let mut keys: Vec<String> = self
						.kv
						.keys()
						.filter(|key| key.starts_with(prefix.as_str()))
						.cloned()
						.collect();
					let truncated = keys.len() > limit;
					keys.truncate(limit);
					ok(id, ResponseValue::KvKeys { keys, truncated })
				}
				HostRequest::AttachmentRead {
					id,
					attachment_id,
					max_bytes,
				} => match self.attachments.get(attachment_id) {
					Some((filename, content_type, bytes)) => {
						let limit = max_bytes.unwrap_or(MAX_ATTACHMENT_READ_BYTES) as usize;
						let read = &bytes[..bytes.len().min(limit)];
						let (encoding, data) = match std::str::from_utf8(read) {
							Ok(text) => (ContentEncoding::Text, text.to_owned()),
							Err(_) => (ContentEncoding::Base64, base64(read)),
						};
						ok(
							id,
							ResponseValue::Attachment {
								content: AttachmentContent {
									attachment_id: attachment_id.clone(),
									filename: filename.clone(),
									content_type: content_type.clone(),
									size: bytes.len() as u64,
									encoding,
									data,
									truncated: read.len() < bytes.len(),
								},
							},
						)
					}
					None => missing(id),
				},
				HostRequest::Http { id, request } => match self.web.get(&request.url) {
					Some((status, body)) => ok(
						id,
						ResponseValue::Http {
							response: Box::new(HttpResponse {
								status: *status,
								content_type: None,
								location: None,
								encoding: ContentEncoding::Text,
								body: body.clone(),
								truncated: false,
							}),
						},
					),
					None => missing(id),
				},
				HostRequest::LogRead { id, before, limit } => {
					let limit = limit.unwrap_or(MAX_LOG_READ) as usize;
					let mut entries: Vec<LogEntry> = self
						.log
						.iter()
						.rev()
						.filter(|entry| before.is_none_or(|before| entry.seq < before))
						.take(limit + 1)
						.cloned()
						.collect();
					let more = entries.len() > limit;
					entries.truncate(limit);
					ok(id, ResponseValue::LogEntries { entries, more })
				}
				HostRequest::ServerProfile { id, guild_id } => match self.servers.get(guild_id) {
					Some(profile) => ok(
						id,
						ResponseValue::ServerProfile {
							profile: Box::new(profile.clone()),
						},
					),
					None => missing(id),
				},
			}
		}
	}

	fn base64(bytes: &[u8]) -> String {
		const TABLE: &[u8; 64] =
			b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
		let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
		for chunk in bytes.chunks(3) {
			let n = (u32::from(chunk[0]) << 16)
				| (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
				| u32::from(*chunk.get(2).unwrap_or(&0));
			for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
				if index <= chunk.len() {
					out.push(TABLE[(n >> shift & 63) as usize] as char);
				} else {
					out.push('=');
				}
			}
		}
		out
	}
}
