//! API v2 hooks, notifications and logs.
//!
//! * `before_send` actions see each outgoing message and may return replacement text. Harmony
//!   gives them a short budget and sends the original text if anything goes wrong.
//! * Event actions may return `notifications`, shown as system notifications, rate limited.
//! * `log` appends lines to the extension's own log file, bounded by its log quota (oldest
//!   lines are dropped first); `log_read` pages back through it.
use crate::{Capability, Error, Manifest, Surface};
use serde::{Deserialize, Serialize};

/// Longest a `before_send` chain may hold a message, in milliseconds.
pub const SEND_HOOK_BUDGET_MS: u64 = 1500;
/// The hold when a hook's extension has `network`: its hook may make `http` requests, such as
/// translating the message before it is sent.
pub const SEND_HOOK_NETWORK_BUDGET_MS: u64 = 5000;
/// Most `before_send` extensions run for one message, in a stable order.
pub const MAX_SEND_HOOKS: usize = 4;
/// Outgoing text a hook may return, in bytes (Discord's limit is far lower; the host checks it).
pub const MAX_SEND_TEXT: usize = 16 * 1024;

pub const MAX_NOTIFICATIONS_PER_OUTPUT: usize = 3;
pub const MAX_NOTIFICATION_TITLE: usize = 128;
pub const MAX_NOTIFICATION_BODY: usize = 512;
/// One extension may show at most this many notifications per minute.
pub const NOTIFICATIONS_PER_MINUTE: usize = 6;

pub const MAX_LOG_LINES_PER_OUTPUT: usize = 32;
pub const MAX_LOG_LINE: usize = 8 * 1024;
pub const MAX_LOG_READ: u32 = 200;

/// A system notification an event action asks Harmony to show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notification {
	pub title: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub body: String,
	/// Opens this conversation when the notification is clicked.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub channel_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub message_id: Option<String>,
}

/// One line of the extension's log, as `log_read` returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
	/// Increases by one for every appended line; starts again at 1 after the log is cleared.
	pub seq: u64,
	/// Unix milliseconds when Harmony stored the line.
	pub at_ms: u64,
	pub text: String,
}

fn plain(text: &str, max: usize) -> bool {
	text.len() <= max && !text.chars().any(|c| c.is_control() && c != '\n')
}

impl Notification {
	fn validate(&self) -> Result<(), Error> {
		if self.title.trim().is_empty()
			|| !plain(&self.title, MAX_NOTIFICATION_TITLE)
			|| self.title.contains('\n')
			|| !plain(&self.body, MAX_NOTIFICATION_BODY)
		{
			return Err(Error::Invalid);
		}
		for id in self.channel_id.iter().chain(&self.message_id) {
			crate::app::entity_id(id)?;
		}
		if self.message_id.is_some() && self.channel_id.is_none() {
			return Err(Error::Invalid);
		}
		Ok(())
	}
}

/// Manifest rules: hooks need `send_hook`, logs need a log quota.
pub(crate) fn validate_manifest(manifest: &Manifest) -> Result<(), Error> {
	let hooks = manifest
		.actions
		.iter()
		.filter(|action| action.surface == Surface::BeforeSend)
		.count();
	if manifest
		.actions
		.iter()
		.filter(|action| action.surface == Surface::GifSearch)
		.count()
		> 1
	{
		return Err(Error::Invalid);
	}
	if hooks > 1 || hooks == 1 && !manifest.capabilities.contains(&Capability::SendHook) {
		return Err(Error::Capability);
	}
	if manifest
		.capabilities
		.contains(&Capability::AllMessageEvents)
		&& !manifest.capabilities.contains(&Capability::MessageEvents)
	{
		return Err(Error::Capability);
	}
	Ok(())
}

/// Output rules for hooks, notifications and log lines.
pub(crate) fn validate_output(
	output: &crate::Output,
	manifest: &Manifest,
	surface: Surface,
) -> Result<(), Error> {
	let v2 = manifest.api_version >= crate::API_V2;
	if !v2 && (!output.notifications.is_empty() || !output.log.is_empty() || output.log_clear) {
		return Err(Error::Invalid);
	}
	if surface == Surface::BeforeSend
		&& (!output.panel.is_empty()
			|| !output.effects.is_empty()
			|| output.storage.is_some()
			|| !output.kv.is_empty()
			// Hooks may fetch (`http`, answered inside the worker), but not reach other data.
			|| output
				.requests
				.iter()
				.any(|request| !matches!(request, crate::v2::HostRequest::Http { .. }))
			|| !output.contributions.is_empty()
			|| !output.notifications.is_empty()
			|| output.appearance.is_some()
			|| output
				.replacement
				.as_ref()
				.is_some_and(|text| text.len() > MAX_SEND_TEXT))
	{
		return Err(Error::Capability);
	}
	if !output.notifications.is_empty() {
		if !manifest.capabilities.contains(&Capability::Notifications)
			|| !matches!(
				surface,
				Surface::MessageEvent | Surface::AppEvent | Surface::Tick
			) {
			return Err(Error::Capability);
		}
		if output.notifications.len() > MAX_NOTIFICATIONS_PER_OUTPUT {
			return Err(Error::Limit);
		}
		output
			.notifications
			.iter()
			.try_for_each(Notification::validate)?;
	}
	if !output.log.is_empty() || output.log_clear {
		if crate::v2::quotas_of(manifest).log_bytes == 0
			|| !manifest.capabilities.contains(&Capability::Storage)
		{
			return Err(Error::Capability);
		}
		if output.log.len() > MAX_LOG_LINES_PER_OUTPUT {
			return Err(Error::Limit);
		}
		if output
			.log
			.iter()
			.any(|line| line.is_empty() || !plain(line, MAX_LOG_LINE))
		{
			return Err(Error::Invalid);
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{Invocation, Output};

	fn manifest(capabilities: &str, actions: &str, quotas: &str) -> Manifest {
		serde_json::from_str(&format!(
			r#"{{"api_version":2,"id":"probe","name":"Probe","version":"1.0.0","author":"Synthetic",
			"license":"MIT","source":"https://example.org/probe","kind":"plugin",
			"capabilities":[{capabilities}],"actions":[{actions}]{quotas}}}"#
		))
		.unwrap()
	}
	const EVENT: &str = r#"{"id":"event","label":"Event","surface":"message_event"}"#;
	const HOOK: &str = r#"{"id":"hook","label":"Hook","surface":"before_send"}"#;
	const PANEL: &str = r#"{"id":"panel","label":"Panel","surface":"panel"}"#;

	fn notification() -> Notification {
		Notification {
			title: "Hi".into(),
			body: String::new(),
			channel_id: Some("5".into()),
			message_id: None,
		}
	}

	#[test]
	fn hooks_notifications_and_logs_need_their_grants_and_places() {
		// Hooks need `send_hook`; all-conversation events need `message_events`.
		assert!(manifest(r#""send_hook""#, HOOK, "").validate().is_ok());
		assert!(manifest("", HOOK, "").validate().is_err());
		assert!(
			manifest(r#""all_message_events""#, PANEL, "")
				.validate()
				.is_err()
		);
		let hook = manifest(r#""send_hook""#, HOOK, "");
		let output = |f: fn(&mut Output)| {
			let mut output = Output {
				replacement: Some("text".into()),
				..Default::default()
			};
			f(&mut output);
			output
		};
		let surface = Surface::BeforeSend;
		assert!(validate_output(&output(|_| {}), &hook, surface).is_ok());
		// A hook may only change the text: no panels, effects or storage.
		assert!(
			validate_output(
				&output(|o| o.panel = vec![crate::Element::Separator]),
				&hook,
				surface
			)
			.is_err()
		);
		assert!(
			validate_output(&output(|o| o.storage = Some("x".into())), &hook, surface).is_err()
		);

		// Notifications: event actions with the grant only.
		let events = manifest(
			r#""message_events","notifications""#,
			&format!("{EVENT},{PANEL}"),
			"",
		);
		let notify = Output {
			notifications: vec![notification()],
			..Default::default()
		};
		assert!(validate_output(&notify, &events, Surface::MessageEvent).is_ok());
		assert!(validate_output(&notify, &events, Surface::Panel).is_err());
		let ungranted = manifest(r#""message_events""#, EVENT, "");
		assert!(validate_output(&notify, &ungranted, Surface::MessageEvent).is_err());
		let mut many = notify.clone();
		many.notifications = vec![notification(); MAX_NOTIFICATIONS_PER_OUTPUT + 1];
		assert!(validate_output(&many, &events, Surface::MessageEvent).is_err());

		// Log lines need `storage` and a declared log quota.
		let log = Output {
			log: vec!["line".into()],
			..Default::default()
		};
		let quota = r#","quotas":{"log_bytes":1024}"#;
		assert!(
			validate_output(
				&log,
				&manifest(r#""storage""#, PANEL, quota),
				Surface::Panel
			)
			.is_ok()
		);
		assert!(
			validate_output(&log, &manifest(r#""storage""#, PANEL, ""), Surface::Panel).is_err()
		);
		assert!(validate_output(&log, &manifest("", PANEL, quota), Surface::Panel).is_err());

		// The outgoing text reaches a hook under `send_hook`, not the composer grant.
		let input = Invocation {
			action: "hook".into(),
			composer: Some("text".into()),
			..Default::default()
		};
		assert!(input.validate(&hook).is_ok());
	}
}
