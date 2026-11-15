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
