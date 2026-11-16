//! API v2 storage and request handling on the extension worker: host-built settings, secret
//! presence, the key-value store, local request answers and dependency information.
//! Nothing here runs on the UI thread.
use super::{Gate, atomic_write, read_bounded, remove_file};
use extensions::{
	AttachmentContent, ContentEncoding, EffectiveQuotas, HostRequest, HostResponse, Invocation,
	KvOp, Manifest, RequestErrorCode, ResourceLimits, ResponseValue, SettingType, SettingValue,
};
use std::{collections::BTreeMap, path::Path, time::Duration};

const SETTINGS_FILE: &str = "settings.json";
/// IDs of the secrets the user has set. Values live only in the OS credential store.
const SECRETS_FILE: &str = "secrets.json";
const KV_FILE: &str = "kv.json";
/// One JSON `LogEntry` per line; the newest lines are at the end.
const LOG_FILE: &str = "log.jsonl";
/// Hosts that serve Discord message attachments.
const ATTACHMENT_HOSTS: &[&str] = &["cdn.discordapp.com", "media.discordapp.net"];

pub(super) fn quotas(manifest: &Manifest) -> EffectiveQuotas {
	manifest.quotas.effective(&ResourceLimits::DEFAULT)
}

/// Saved settings with defaults filled in. Unreadable or outdated values fall back to defaults.
pub(super) fn settings(directory: &Path, manifest: &Manifest) -> BTreeMap<String, SettingValue> {
	let saved = read_bounded(
		&directory.join(SETTINGS_FILE),
		quotas(manifest).settings_bytes as usize,
	)
	.ok()
	.and_then(|bytes| serde_json::from_slice(&bytes).ok())
	.unwrap_or_default();
	extensions::resolve_settings(&manifest.settings, &saved)
}

pub(super) fn save_settings(
	directory: &Path,
	manifest: &Manifest,
	values: &BTreeMap<String, SettingValue>,
	gate: &Gate,
) -> Result<BTreeMap<String, SettingValue>, String> {
	extensions::validate_saved_settings(
		&manifest.settings,
		values,
		quotas(manifest).settings_bytes,
	)
	.map_err(|error| match error {
		extensions::Error::Limit => "Settings exceed this extension's settings quota".to_owned(),
		_ => "A setting value is not allowed by this extension".to_owned(),
	})?;
	let bytes = serde_json::to_vec(values).map_err(|_| "Cannot encode settings")?;
	atomic_write(&directory.join(SETTINGS_FILE), &bytes, gate)?;
	Ok(settings(directory, manifest))
}

fn secret_ids(directory: &Path) -> Vec<String> {
	read_bounded(&directory.join(SECRETS_FILE), 4096)
		.ok()
		.and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
		.unwrap_or_default()
}

/// Whether each declared secret has been set.
pub(super) fn secret_presence(directory: &Path, manifest: &Manifest) -> BTreeMap<String, bool> {
	let set = secret_ids(directory);
	manifest
		.settings
		.iter()
		.filter(|field| field.kind == SettingType::Secret)
		.map(|field| (field.id.clone(), set.contains(&field.id)))
		.collect()
}

/// Credential-store entry name. The scope is the account's directory name (a digest), so a
/// secret never names the account and two accounts never share one.
fn secret_name(scope: &Path, id: &str, field: &str) -> Result<String, String> {
	let scope = scope
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or("Invalid extension scope")?;
	Ok(format!("extension-secret.{scope}.{id}.{field}"))
}

/// Stores or clears one secret. `directory` is the extension's own directory.
pub(super) fn save_secret(
	directory: &Path,
	manifest: &Manifest,
	field: &str,
	value: Option<&str>,
	gate: &Gate,
) -> Result<BTreeMap<String, bool>, String> {
	let scope = directory.parent().ok_or("Invalid extension directory")?;
	let name = secret_name(scope, &manifest.id, field)?;
	let mut set = secret_ids(directory);
	match value {
		Some(value) => {
			extensions::validate_secret(&manifest.settings, field, value)
				.map_err(|_| "This secret is not declared by the extension or is too long")?;
			platform::save_extension_secret(&name, value).map_err(|error| match error {
				platform::CredentialError::NoStore => {
					"No OS credential store is available, so secrets cannot be saved".to_owned()
				}
				_ => "The OS credential store refused the secret; unlock it and retry".to_owned(),
			})?;
			if !set.iter().any(|id| id == field) {
				set.push(field.to_owned());
			}
		}
		None => {
			if !manifest
				.settings
				.iter()
				.any(|setting| setting.id == field && setting.kind == SettingType::Secret)
			{
				return Err("This secret is not declared by the extension".into());
			}
			platform::forget_extension_secret(&name)
				.map_err(|_| "The OS credential store could not remove the secret; retry")?;
			set.retain(|id| id != field);
		}
	}
	gate.check()?;
	let bytes = serde_json::to_vec(&set).map_err(|_| "Cannot encode secret list")?;
	atomic_write(&directory.join(SECRETS_FILE), &bytes, gate)?;
	Ok(secret_presence(directory, manifest))
}

/// Removes every secret an extension stored. Called before its directory is deleted, so a
/// failure leaves the directory (and the list of secrets) in place for the retry.
pub(super) fn forget_secrets(directory: &Path) -> Result<(), String> {
	let Some(scope) = directory.parent() else {
		return Ok(());
	};
	let Some(id) = directory.file_name().and_then(|name| name.to_str()) else {
		return Ok(());
	};
	let mut remaining = secret_ids(directory);
	if remaining.is_empty() {
		return Ok(());
	}
	remaining.retain(|field| {
		secret_name(scope, id, field)
			.is_ok_and(|name| platform::forget_extension_secret(&name).is_err())
	});
	if remaining.is_empty() {
		remove_file(&directory.join(SECRETS_FILE))
	} else {
		Err("The OS credential store could not remove this extension's secrets; retry".into())
	}
}

fn read_kv(directory: &Path, quota: u64) -> BTreeMap<String, String> {
	read_bounded(&directory.join(KV_FILE), quota as usize + 2)
		.ok()
		.and_then(|bytes| serde_json::from_slice(&bytes).ok())
		.unwrap_or_default()
}

/// Bytes the key-value store uses, as counted against the storage quota.
pub(super) fn kv_bytes(directory: &Path) -> u64 {
	std::fs::metadata(directory.join(KV_FILE)).map_or(0, |metadata| metadata.len())
}

/// Applies the changes in order. Nothing is written if the result would exceed the quota.
pub(super) fn apply_kv(
	directory: &Path,
	manifest: &Manifest,
	ops: &[KvOp],
	gate: &Gate,
) -> Result<(), String> {
	if ops.is_empty() {
		return Ok(());
	}
	let quota = quotas(manifest).storage_bytes;
	let mut store = read_kv(directory, quota);
	for op in ops {
		match op {
			KvOp::Set { key, value } => {
				store.insert(key.clone(), value.clone());
			}
			KvOp::Delete { key } => {
				store.remove(key);
			}
			KvOp::Clear => store.clear(),
		}
	}
	let bytes = serde_json::to_vec(&store).map_err(|_| "Cannot encode extension storage")?;
	if bytes.len() as u64 > quota {
		return Err(format!(
			"Extension storage would use {} bytes; its quota is {quota} bytes",
			bytes.len()
		));
	}
	gate.check()?;
	if store.is_empty() {
		remove_file(&directory.join(KV_FILE))
	} else {
		atomic_write(&directory.join(KV_FILE), &bytes, gate)
	}
}

fn read_log(directory: &Path, quota: u64) -> Vec<extensions::LogEntry> {
	read_bounded(&directory.join(LOG_FILE), quota.max(1) as usize)
		.ok()
		.map(|bytes| {
			bytes
				.split(|b| *b == b'\n')
				// A line cut short by a crash is skipped, not treated as corrupt data.
				.filter_map(|line| serde_json::from_slice::<extensions::LogEntry>(line).ok())
				.collect()
		})
		.unwrap_or_default()
}

/// Appends lines to the extension's log, dropping the oldest lines to stay within its log
/// quota. `clear` empties it first.
pub(super) fn apply_log(
	directory: &Path,
	manifest: &Manifest,
	lines: &[String],
	clear: bool,
	gate: &Gate,
) -> Result<(), String> {
	if lines.is_empty() && !clear {
		return Ok(());
	}
	let quota = quotas(manifest).log_bytes;
	if quota == 0 {
		return Err("This extension has no log quota".into());
	}
	let mut entries = if clear {
		Vec::new()
	} else {
		read_log(directory, quota)
	};
	let first = entries.last().map_or(0, |entry| entry.seq) + 1;
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_millis() as u64);
	for (seq, line) in (first..).zip(lines) {
		entries.push(extensions::LogEntry {
			seq,
			at_ms: now,
			text: line.clone(),
		});
	}
	let encoded: Vec<Vec<u8>> = entries
		.iter()
		.filter_map(|entry| serde_json::to_vec(entry).ok())
		.collect();
	// Keep the newest lines that fit; trim to three quarters so appends are not rewritten
	// on every call once the log is full.
	let mut total = encoded
		.iter()
		.map(|line| line.len() as u64 + 1)
		.sum::<u64>();
	let mut start = 0;
	if total > quota {
		while start < encoded.len() && total > quota / 4 * 3 {
			total -= encoded[start].len() as u64 + 1;
			start += 1;
		}
	}
	let mut bytes = Vec::with_capacity(total as usize);
	for line in &encoded[start..] {
		bytes.extend_from_slice(line);
		bytes.push(b'\n');
	}
	gate.check()?;
	if bytes.is_empty() {
		remove_file(&directory.join(LOG_FILE))
	} else {
		atomic_write(&directory.join(LOG_FILE), &bytes, gate)
	}
}

/// Answers a request the worker can serve itself, or `None` when the app must answer it.
pub(super) fn answer_local(
	directory: &Path,
	manifest: &Manifest,
	invocation: &Invocation,
	request: &HostRequest,
	gate: &Gate,
) -> Option<HostResponse> {
	let quota = quotas(manifest).storage_bytes;
	Some(match request {
		HostRequest::KvGet { id, keys } => {
			let store = read_kv(directory, quota);
			HostResponse::ok(
				id,
				ResponseValue::Kv {
					values: keys
						.iter()
						.map(|key| (key.clone(), store.get(key).cloned()))
						.collect(),
				},
			)
		}
		HostRequest::KvList { id, prefix, limit } => {
			let store = read_kv(directory, quota);
			let limit = limit.unwrap_or(extensions::MAX_KV_LIST) as usize;
			let mut keys: Vec<String> = store
				.keys()
				.filter(|key| key.starts_with(prefix.as_str()))
				.take(limit + 1)
				.cloned()
				.collect();
			let truncated = keys.len() > limit;
			keys.truncate(limit);
			HostResponse::ok(id, ResponseValue::KvKeys { keys, truncated })
		}
		HostRequest::AttachmentRead {
			id,
			attachment_id,
			max_bytes,
		} => read_attachment(invocation, id, attachment_id, *max_bytes, gate),
		HostRequest::ServerProfile { .. } => return None,
		HostRequest::Http { id, request } => http(directory, manifest, id, request, gate),
		HostRequest::LogRead { id, before, limit } => {
			let entries = read_log(directory, quotas(manifest).log_bytes);
			let limit = limit.unwrap_or(extensions::MAX_LOG_READ) as usize;
			let mut older: Vec<_> = entries
				.into_iter()
				.rev()
				.filter(|entry| before.is_none_or(|before| entry.seq < before))
				.take(limit + 1)
				.collect();
			let more = older.len() > limit;
			older.truncate(limit);
			HostResponse::ok(
				id,
				ResponseValue::LogEntries {
					entries: older,
					more,
				},
			)
		}
	})
}

fn read_attachment(
	invocation: &Invocation,
	id: &str,
	attachment_id: &str,
	max_bytes: Option<u32>,
	gate: &Gate,
) -> HostResponse {
	let Some(source) = invocation
		.attachment_sources
		.iter()
		.find(|source| source.id == attachment_id)
	else {
		return HostResponse::error(
			id,
			RequestErrorCode::NotFound,
			"That attachment is not on the message this action was chosen on",
		);
	};
	let allowed = url::Url::parse(&source.url).is_ok_and(|url| {
		url.scheme() == "https"
			&& url
				.host_str()
				.is_some_and(|host| ATTACHMENT_HOSTS.contains(&host))
	});
	if !allowed {
		return HostResponse::error(
			id,
			RequestErrorCode::Unsupported,
			"Only Discord-hosted attachments can be read",
		);
	}
	let limit = max_bytes.unwrap_or(extensions::MAX_ATTACHMENT_READ_BYTES) as usize;
	match super::download_prefix(&source.url, limit, gate, Duration::from_secs(30)) {
		Ok((bytes, truncated)) => {
			let (encoding, data) = match std::str::from_utf8(&bytes) {
				Ok(text) => (ContentEncoding::Text, text.to_owned()),
				Err(error) if truncated && error.error_len().is_none() => {
					// A cut in the middle of a character: keep the complete text before it.
					let valid = error.valid_up_to();
					(
						ContentEncoding::Text,
						String::from_utf8_lossy(&bytes[..valid]).into_owned(),
					)
				}
				Err(_) => (ContentEncoding::Base64, base64(&bytes)),
			};
			HostResponse::ok(
				id,
				ResponseValue::Attachment {
					content: AttachmentContent {
						attachment_id: source.id.clone(),
						filename: source.filename.clone(),
						content_type: source.content_type.clone(),
						size: source.size,
						encoding,
						data,
						truncated,
					},
				},
			)
		}
		Err(error) => HostResponse::error(id, RequestErrorCode::Failed, error),
	}
}

fn base64(bytes: &[u8]) -> String {
	const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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

/// Every other installed extension in `parent`, for dependency and conflict checks.
pub(super) fn installed_manifests(parent: &Path, except: &str) -> Vec<Manifest> {
	let Ok(entries) = std::fs::read_dir(parent) else {
		return Vec::new();
	};
	entries
		.filter_map(Result::ok)
		.take(ResourceLimits::DEFAULT.installed_plugins as usize * 2)
		.filter(|entry| entry.path().is_dir() && entry.file_name() != except)
		.filter(|entry| !entry.path().with_extension("disabled").exists())
		.filter_map(|entry| super::read_stored(&entry.path().join("package.json")).ok())
		.map(|stored| stored.package.manifest)
		.collect()
}

/// Checks required dependencies and conflicts against the other enabled extensions.
pub(super) fn check_dependencies(manifest: &Manifest, others: &[Manifest]) -> Result<(), String> {
	let installed: Vec<_> = others
		.iter()
		.map(|other| extensions::InstalledVersion {
			id: &other.id,
			version: &other.version,
			conflicts: &other.conflicts,
		})
		.collect();
	extensions::check_dependencies(manifest, &installed)
}

/// Total package bytes installed in `parent`, for the scope's byte budget.
pub(super) fn installed_bytes(parent: &Path) -> u64 {
	let Ok(entries) = std::fs::read_dir(parent) else {
		return 0;
	};
	entries
		.filter_map(Result::ok)
		.take(ResourceLimits::DEFAULT.installed_plugins as usize * 2)
		.filter_map(|entry| std::fs::metadata(entry.path().join("package.json")).ok())
		.map(|metadata| metadata.len())
		.sum()
}

// ---- Network requests (API v2 `http`) ----

/// Recent request times per extension, for the per-minute limit.
static REQUESTS: std::sync::Mutex<
	BTreeMap<String, std::collections::VecDeque<std::time::Instant>>,
> = std::sync::Mutex::new(BTreeMap::new());

/// Hosts this extension may contact now: fixed origins, plus the hosts of URLs the user typed
/// into the settings that `setting:` origins name.
fn allowed_hosts(directory: &Path, manifest: &Manifest) -> Vec<String> {
	let values = settings(directory, manifest);
	manifest
		.origins
		.iter()
		.filter_map(|origin| match origin.strip_prefix("setting:") {
			Some(id) => match values.get(id) {
				Some(SettingValue::Text(text)) => url::Url::parse(text.trim())
					.ok()
					.filter(|url| url.scheme() == "https")
					.and_then(|url| url.host_str().map(str::to_ascii_lowercase))
					.filter(|host| extensions::valid_origin_host(host)),
				_ => None,
			},
			None => Some(origin.clone()),
		})
		.collect()
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
	let value = |b: u8| match b {
		b'A'..=b'Z' => Some(b - b'A'),
		b'a'..=b'z' => Some(b - b'a' + 26),
		b'0'..=b'9' => Some(b - b'0' + 52),
		b'+' => Some(62),
		b'/' => Some(63),
		_ => None,
	};
	let trimmed = text.trim_end_matches('=');
	let mut out = Vec::with_capacity(trimmed.len() * 3 / 4);
	let (mut buffer, mut bits) = (0u32, 0u32);
	for byte in trimmed.bytes() {
		buffer = (buffer << 6) | u32::from(value(byte)?);
		bits += 6;
		if bits >= 8 {
			bits -= 8;
			out.push((buffer >> bits) as u8);
		}
	}
	Some(out)
}

/// Makes one extension request, or explains why it was refused. Never follows redirects.
fn http(
	directory: &Path,
	manifest: &Manifest,
	id: &str,
	request: &extensions::HttpRequest,
	gate: &Gate,
) -> HostResponse {
	let refuse = |code, message: &str| HostResponse::error(id, code, message);
	let Ok(mut url) = url::Url::parse(&request.url) else {
		return refuse(RequestErrorCode::Invalid, "The address is not a valid URL");
	};
	let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
	if url.scheme() != "https"
		|| !url.username().is_empty()
		|| url.password().is_some()
		|| url.port().is_some_and(|port| port != 443)
		|| !allowed_hosts(directory, manifest).contains(&host)
	{
		return refuse(
			RequestErrorCode::NotGranted,
			"That site is not one this extension may contact",
		);
	}
	{
		let now = std::time::Instant::now();
		let mut recent = REQUESTS
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner());
		let times = recent.entry(manifest.id.clone()).or_default();
		times.retain(|at| now.duration_since(*at) < Duration::from_secs(60));
		if times.len() >= extensions::REQUESTS_PER_MINUTE {
			return refuse(RequestErrorCode::Limit, "Too many requests; wait a minute");
		}
		times.push_back(now);
	}
	let Some(scope) = directory.parent() else {
		return refuse(RequestErrorCode::Failed, "Extension folder is unavailable");
	};
	let mut headers = request.headers.clone();
	for secret in &request.secret_headers {
		let value = secret_name(scope, &manifest.id, &secret.secret)
			.ok()
			.and_then(|name| platform::load_extension_secret(&name).ok().flatten());
		let Some(value) = value else {
			return refuse(
				RequestErrorCode::Unavailable,
				&format!(
					"Set the {} secret in this extension's settings",
					secret.secret
				),
			);
		};
		headers.push((secret.header.clone(), format!("{}{value}", secret.prefix)));
	}
	for secret in &request.secret_params {
		let value = secret_name(scope, &manifest.id, &secret.secret)
			.ok()
			.and_then(|name| platform::load_extension_secret(&name).ok().flatten());
		let Some(value) = value else {
			return refuse(
				RequestErrorCode::Unavailable,
				&format!(
					"Set the {} secret in this extension's settings",
					secret.secret
				),
			);
		};
		url.query_pairs_mut().append_pair(&secret.param, &value);
	}
	let body = match &request.body {
		Some(text) if request.body_base64 => match decode_base64(text) {
			Some(bytes) => Some(bytes),
			None => return refuse(RequestErrorCode::Invalid, "The body is not valid base64"),
		},
		Some(text) => Some(text.clone().into_bytes()),
		None => None,
	};
	let limit = (quotas(manifest).network_response_bytes as usize)
		.min(request.max_bytes.map_or(usize::MAX, |max| max as usize))
		.max(1);
	match super::extension_http(&url, request.method, &headers, body, limit, gate) {
		Ok((status, content_type, location, bytes, truncated)) => {
			let (encoding, body) = match String::from_utf8(bytes) {
				Ok(text) => (extensions::ContentEncoding::Text, text),
				Err(error) => (
					extensions::ContentEncoding::Base64,
					base64(error.as_bytes()),
				),
			};
			HostResponse::ok(
				id,
				ResponseValue::Http {
					response: Box::new(extensions::HttpResponse {
						status,
						content_type,
						location,
						encoding,
						body,
						truncated,
					}),
				},
			)
		}
		Err(message) => refuse(RequestErrorCode::Failed, &message),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn gate() -> Gate {
		Gate {
			epoch: 0,
			generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
			wake_cancel: std::sync::Arc::new(tokio::sync::Notify::new()),
		}
	}

	/// Only listed hosts, and the host of a server the user typed into a setting, may be
	/// contacted; anything else is refused before any connection is made.
	#[test]
	fn requests_go_only_to_listed_and_user_chosen_sites() {
		let directory =
			std::env::temp_dir().join(format!("harmony-network-check-{}", std::process::id()));
		let scoped = directory.join("account").join("probe");
		std::fs::create_dir_all(&scoped).unwrap();
		let manifest: Manifest = serde_json::from_str(
			r#"{"api_version":2,"id":"probe","name":"Probe","version":"1.0.0","author":"Synthetic",
			"license":"MIT","source":"https://example.org/probe","kind":"plugin",
			"capabilities":["network"],"origins":["api.example.org","setting:server"],
			"settings":[{"id":"server","label":"Server","type":"text","max_chars":200}],
			"actions":[{"id":"open","label":"Open","surface":"panel"}]}"#,
		)
		.unwrap();
		assert!(manifest.validate().is_ok());
		let refused = |url: &str| {
			let request = extensions::HttpRequest {
				method: extensions::HttpMethod::Get,
				url: url.into(),
				headers: Vec::new(),
				secret_headers: Vec::new(),
				secret_params: Vec::new(),
				body: None,
				body_base64: false,
				max_bytes: None,
			};
			http(&scoped, &manifest, "r", &request, &gate())
				.error
				.map(|error| error.code)
		};
		for url in [
			"https://evil.example.com/",
			"http://api.example.org/",
			"https://api.example.org:8443/",
			"https://user:pass@api.example.org/",
			"https://files.example.net/upload",
		] {
			assert_eq!(refused(url), Some(RequestErrorCode::NotGranted), "{url}");
		}
		save_settings(
			&scoped,
			&manifest,
			&BTreeMap::from([(
				"server".into(),
				SettingValue::Text("https://files.example.net/base".into()),
			)]),
			&gate(),
		)
		.unwrap();
		assert_eq!(
			allowed_hosts(&scoped, &manifest),
			["api.example.org", "files.example.net"]
		);
		std::fs::remove_dir_all(&directory).unwrap();
	}
}
