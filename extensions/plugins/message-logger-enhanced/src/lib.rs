//! Message Logger Enhanced: keeps a log of deleted and edited messages, shows earlier versions
//! under edited messages, and flags ghost pings (a message that mentioned you, then vanished).
//! Messages are remembered from the open conversation as they arrive; deleted-message events
//! carry no text, so the logger can only log what it saw. Written for Harmony using only the
//! public API v2 SDK: message events, key-value storage, the log, contributions and
//! notifications.
use harmony_extension_sdk::{
	ContributionIcon, ContributionItem, ContributionOp, ContributionTarget, Element,
	ExtensionInvocation, ExtensionOutput, HostRequest, MessageEventKind, Notification,
	ResponseValue, Slot, serde_json,
};
use serde::{Deserialize, Serialize};

/// Messages remembered at once; the oldest are forgotten first.
const REMEMBERED: u32 = 250;
/// Text kept per remembered message, in characters.
const KEPT_TEXT: usize = 2000;
/// Log lines shown per page in the viewer.
const PAGE: u32 = 30;

/// A message the logger has seen, keyed `m/<zero-padded id>` so keys sort oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
	#[serde(rename = "a")]
	pub author: String,
	#[serde(rename = "p", default)]
	pub place: String,
	#[serde(rename = "c")]
	pub content: String,
	#[serde(rename = "m", default)]
	pub mentions_me: bool,
}

/// One log line, stored as JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logged {
	/// `deleted`, `edited` or `ghost`.
	pub kind: String,
	pub author: String,
	pub place: String,
	pub content: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub before: String,
	pub channel_id: String,
	pub message_id: String,
}

fn key(message_id: &str) -> String {
	format!("m/{message_id:0>20}")
}

fn kept(text: &str) -> String {
	text.chars().take(KEPT_TEXT).collect()
}

fn remembered(input: &ExtensionInvocation, id: &str) -> Option<Seen> {
	match input
		.response(id)
		.and_then(|response| response.value.as_ref())
	{
		Some(ResponseValue::Kv { values }) => values
			.values()
			.next()
			.cloned()
			.flatten()
			.and_then(|text| serde_json::from_str(&text).ok()),
		_ => None,
	}
}

fn line(entry: &Logged) -> String {
	serde_json::to_string(entry).unwrap_or_default()
}

fn on_message(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut output = ExtensionOutput::default();
	let Some(event) = input.message.clone() else {
		return output;
	};
	let slot = key(&event.message_id);
	match event.kind {
		MessageEventKind::Create => {
			if input.round == 0 {
				let seen = Seen {
					author: event.author_name.clone().unwrap_or_default(),
					place: event.place.clone().unwrap_or_default(),
					content: kept(event.content.as_deref().unwrap_or_default()),
					mentions_me: event.mentions_me,
				};
				output.kv_set(slot, serde_json::to_string(&seen).unwrap_or_default());
				output.request(HostRequest::KvList {
					id: "seen".into(),
					prefix: "m/".into(),
					limit: Some(REMEMBERED),
				});
			} else if let Some(ResponseValue::KvKeys {
				keys,
				truncated: true,
			}) = input.response("seen").and_then(|r| r.value.as_ref())
			{
				// Forget the oldest few at once rather than one per message.
				for old in keys.iter().take(20) {
					output.kv_delete(old.clone());
				}
			}
		}
		MessageEventKind::Update | MessageEventKind::Delete => {
			if input.round == 0 {
				output.request(HostRequest::KvGet {
					id: "old".into(),
					keys: vec![slot],
				});
				return output;
			}
			let Some(before) = remembered(input, "old") else {
				return output;
			};
			let mut entry = Logged {
				author: before.author.clone(),
				place: before.place.clone(),
				channel_id: event.channel_id.clone(),
				message_id: event.message_id.clone(),
				..Default::default()
			};
			if event.kind == MessageEventKind::Delete {
				entry.kind = if before.mentions_me {
					"ghost"
				} else {
					"deleted"
				}
				.into();
				entry.content = before.content.clone();
				output.kv_delete(slot);
				if before.mentions_me && input.setting_bool("ghost-pings") {
					output.notifications.push(Notification {
						title: format!("Ghost ping from {}", or_someone(&before.author)),
						body: format!(
							"A message that mentioned you was deleted in {}",
							before.place
						),
						channel_id: Some(event.channel_id.clone()),
						message_id: None,
					});
				}
			} else {
				let Some(now) = event.content.clone() else {
					return output;
				};
				if now == before.content {
					return output;
				}
				entry.kind = "edited".into();
				entry.before = before.content.clone();
				entry.content = kept(&now);
				let updated = Seen {
					content: kept(&now),
					..before.clone()
				};
				output.kv_set(slot, serde_json::to_string(&updated).unwrap_or_default());
				if input.setting_bool("edits-in-chat") {
					let was: String = before.content.chars().take(280).collect();
					output.contribute(ContributionOp::Set {
						slot: Slot::MessageAccessory,
						key: format!("edit-{}", event.message_id),
						target: ContributionTarget::message(event.message_id.clone()),
						item: ContributionItem::text(format!("Edited · was: {was}"))
							.icon(ContributionIcon::Pin)
							.tooltip("Earlier text, kept by Message Logger"),
						ttl_ms: Some(24 * 60 * 60 * 1000),
					});
				}
			}
			output.log.push(line(&entry));
		}
	}
	output
}

fn or_someone(name: &str) -> &str {
	if name.is_empty() { "someone" } else { name }
}

/// `YYYY-MM-DD HH:MM` (UTC) from Unix milliseconds.
pub fn when(ms: u64) -> String {
	let seconds = ms / 1000;
	let (days, rest) = (seconds / 86_400, seconds % 86_400);
	let z = days as i64 + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let day = doy - (153 * mp + 2) / 5 + 1;
	let month = if mp < 10 { mp + 3 } else { mp - 9 };
	let year = yoe + era * 400 + i64::from(month <= 2);
	format!(
		"{year:04}-{month:02}-{day:02} {:02}:{:02}",
		rest / 3600,
		rest % 3600 / 60
	)
}

fn viewer(input: &ExtensionInvocation) -> ExtensionOutput {
	let mut output = ExtensionOutput::default();
	if input.button() == Some("clear") {
		output.log_clear = true;
		output.output.output.panel = vec![
			Element::Heading {
				text: "Message Logger".into(),
			},
			Element::Text {
				text: "The log is empty.".into(),
			},
		];
		return output;
	}
	let before = input
		.button()
		.and_then(|button| button.strip_prefix("older-"))
		.and_then(|seq| seq.parse::<u64>().ok());
	let query = input
		.value("search")
		.unwrap_or_default()
		.trim()
		.to_lowercase();
	let Some(response) = input.response("page") else {
		output.request(HostRequest::LogRead {
			id: "page".into(),
			before,
			// A search looks further back than one page.
			limit: Some(if query.is_empty() { PAGE } else { 200 }),
		});
		return output;
	};
	let mut panel = vec![
		Element::Heading {
			text: "Message Logger".into(),
		},
		Element::Row {
			children: vec![
				Element::TextInput {
					id: "search".into(),
					label: "Search".into(),
					value: query.clone(),
				},
				Element::Button {
					id: "find".into(),
					label: "Search".into(),
				},
			],
		},
	];
	let Some(ResponseValue::LogEntries { entries, more }) = &response.value else {
		panel.push(Element::Text {
			text: "The log could not be read.".into(),
		});
		output.output.output.panel = panel;
		return output;
	};
	let mut shown = 0;
	for entry in entries {
		let Ok(logged) = serde_json::from_str::<Logged>(&entry.text) else {
			continue;
		};
		let haystack = format!(
			"{} {} {} {}",
			logged.author, logged.place, logged.content, logged.before
		)
		.to_lowercase();
		if !query.is_empty() && !haystack.contains(&query) {
			continue;
		}
		let label = match logged.kind.as_str() {
			"ghost" => "Ghost ping",
			"edited" => "Edited",
			_ => "Deleted",
		};
		panel.push(Element::Separator);
		panel.push(Element::Text {
			text: format!(
				"{label} · {} · {} · {} UTC",
				or_someone(&logged.author),
				logged.place,
				when(entry.at_ms)
			),
		});
		if !logged.before.is_empty() {
			panel.push(Element::Text {
				text: format!("Before: {}", logged.before),
			});
		}
		panel.push(Element::Text {
			text: if logged.kind == "edited" {
				format!("After: {}", logged.content)
			} else {
				logged.content.clone()
			},
		});
		shown += 1;
		if shown >= PAGE {
			break;
		}
	}
	if shown == 0 {
		panel.push(Element::Text {
			text: if query.is_empty() {
				"Nothing logged yet. Deleted and edited messages you saw appear here.".into()
			} else {
				"No logged messages match.".into()
			},
		});
	}
	let mut buttons = Vec::new();
	if *more && let Some(last) = entries.last() {
		buttons.push(Element::Button {
			id: format!("older-{}", last.seq),
			label: "Older".into(),
		});
	}
	buttons.push(Element::Button {
		id: "clear".into(),
		label: "Clear log".into(),
	});
	panel.push(Element::Separator);
	panel.push(Element::Row { children: buttons });
	output.output.output.panel = panel;
	output
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	match input.action() {
		"open" => viewer(&input),
		_ => on_message(&input),
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{MessageEventInfo, SettingValue, testing::TestHost};

	fn event(
		kind: MessageEventKind,
		id: &str,
		content: Option<&str>,
		mentions_me: bool,
	) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "message".into();
		input.message = Some(MessageEventInfo {
			kind,
			channel_id: "30".into(),
			message_id: id.into(),
			author_id: (kind != MessageEventKind::Delete).then(|| "7".into()),
			content: content.map(Into::into),
			author_name: (kind != MessageEventKind::Delete).then(|| "Robin".into()),
			place: Some("Game Night · #general".into()),
			mentions_me,
			open: true,
			..Default::default()
		});
		for (id, on) in [("edits-in-chat", true), ("ghost-pings", true)] {
			input.settings.insert(id.into(), SettingValue::Bool(on));
		}
		input
	}

	#[test]
	fn edits_and_deletes_are_logged_and_ghost_pings_notify() {
		let mut host = TestHost::default();
		host.run(
			event(MessageEventKind::Create, "100", Some("first"), false),
			handle,
		)
		.unwrap();
		host.run(
			event(MessageEventKind::Update, "100", Some("second"), false),
			handle,
		)
		.unwrap();
		host.run(
			event(MessageEventKind::Create, "101", Some("hey <@1>"), true),
			handle,
		)
		.unwrap();
		host.run(event(MessageEventKind::Delete, "101", None, false), handle)
			.unwrap();
		// A delete of a message it never saw logs nothing.
		host.run(event(MessageEventKind::Delete, "999", None, false), handle)
			.unwrap();
		let logged: Vec<Logged> = host
			.log
			.iter()
			.map(|entry| serde_json::from_str(&entry.text).unwrap())
			.collect();
		assert_eq!(logged.len(), 2);
		assert_eq!(
			(
				logged[0].kind.as_str(),
				logged[0].before.as_str(),
				logged[0].content.as_str()
			),
			("edited", "first", "second")
		);
		assert_eq!(
			(logged[1].kind.as_str(), logged[1].content.as_str()),
			("ghost", "hey <@1>")
		);
		assert_eq!(host.notifications.len(), 1);
		assert!(host.notifications[0].title.contains("Robin"));
		assert!(
			host.contributions
				.contains_key(&(Slot::MessageAccessory, "edit-100".into()))
		);
		assert!(!host.kv.contains_key(&key("101")));
	}

	#[test]
	fn the_viewer_searches_pages_and_clears() {
		let mut host = TestHost::default();
		for n in 0..40u64 {
			host.run(
				event(
					MessageEventKind::Create,
					&(200 + n).to_string(),
					Some(&format!("note {n}")),
					false,
				),
				handle,
			)
			.unwrap();
			host.run(
				event(
					MessageEventKind::Delete,
					&(200 + n).to_string(),
					None,
					false,
				),
				handle,
			)
			.unwrap();
		}
		let mut open = ExtensionInvocation::default();
		open.invocation.invocation.invocation.action = "open".into();
		let page = host.run(open.clone(), handle).unwrap();
		let older = page.output.output.panel.iter().any(|element| matches!(element, Element::Row { children } if children.iter().any(|child| matches!(child, Element::Button { id, .. } if id.starts_with("older-")))));
		assert!(older);
		let mut search = open.clone();
		search
			.invocation
			.invocation
			.invocation
			.values
			.insert("search".into(), "note 3".into());
		search.button = Some("find".into());
		let found = host.run(search, handle).unwrap();
		let texts = found
			.output
			.output
			.panel
			.iter()
			.filter(
				|element| matches!(element, Element::Text { text } if text.starts_with("note 3")),
			)
			.count();
		assert_eq!(texts, 11, "note 3 and note 30..39");
		let mut clear = open;
		clear.button = Some("clear".into());
		host.run(clear, handle).unwrap();
		assert!(host.log.is_empty());
	}
}
