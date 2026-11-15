//! Extension API v2: additive manifest fields, declarative settings, resource quotas,
//! dependencies, feature negotiation and host requests.
//!
//! Everything here is optional and only valid in a manifest that declares `api_version: 2`.
//! Version 1 manifests, packages and the Wasm buffer contract are unchanged.
use crate::{Capability, Error, Manifest, valid_https_url, valid_id};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The manifest level that may use the fields in this module.
pub const API_V2: u32 = 2;
/// Manifest levels this host installs. The Wasm buffer contract is the same for every level.
pub const SUPPORTED_API_VERSIONS: &[u32] = &[crate::API_VERSION, API_V2];

/// Host features an extension can require or probe for, with the version this host provides.
/// Versions follow semantic versioning: a minor bump only adds fields or request kinds.
pub const FEATURES: &[(&str, &str)] = &[
	("settings", "1.0.0"),
	("secrets", "1.0.0"),
	("storage.kv", "1.0.0"),
	("host_requests", "1.0.0"),
	("dependencies", "1.0.0"),
	("media.links", "1.0.0"),
	("attachments.read", "1.0.0"),
	("lookups.server_profile", "1.0.0"),
];

/// Capabilities that only an `api_version: 2` manifest may request.
pub const V2_CAPABILITIES: &[Capability] = &[
	Capability::MediaLinks,
	Capability::AttachmentContents,
	Capability::ServerProfiles,
];

pub const MAX_SETTINGS: usize = 32;
pub const MAX_SECRETS: usize = 8;
pub const MAX_SETTING_TEXT_CHARS: u32 = 4096;
pub const MAX_SECRET_CHARS: usize = 1024;
pub const MAX_SETTING_OPTIONS: usize = 32;
pub const MAX_DEPENDENCIES: usize = 16;
pub const MAX_FEATURE_REQUIREMENTS: usize = 32;
pub const MAX_REQUESTS_PER_ROUND: usize = 8;
/// Request/response rounds one user action or event may take before the host stops it.
pub const MAX_REQUEST_ROUNDS: u8 = 4;
pub const MAX_CONTINUATION_BYTES: usize = 8 * 1024;
pub const MAX_KV_KEY_BYTES: usize = 128;
pub const MAX_KV_KEYS_PER_REQUEST: usize = 64;
pub const MAX_KV_LIST: u32 = 256;
pub const MAX_KV_OPS: usize = 64;
pub const MAX_MEDIA_LINKS: usize = 20;
/// Attachment bytes returned in one response; base64 must still fit the 256 KiB buffer.
pub const MAX_ATTACHMENT_READ_BYTES: u32 = 160 * 1024;

/// One resource budget: what a manifest gets without asking and the most it may request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct QuotaLimit {
	pub default: u64,
	pub max: u64,
}

/// Host-wide budgets per resource category. Each extension requests its own amount in
/// `quotas`; the user sees the request when enabling it and the host enforces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ResourceLimits {
	pub settings_bytes: QuotaLimit,
	pub storage_bytes: QuotaLimit,
	pub log_bytes: QuotaLimit,
	pub media_cache_bytes: QuotaLimit,
	pub network_response_bytes: QuotaLimit,
	/// Total size of every installed package in one scope.
	pub installed_package_bytes: u64,
	/// Plugins one account may have installed; packages are also bounded by bytes.
	pub installed_plugins: u32,
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;

impl ResourceLimits {
	pub const DEFAULT: Self = Self {
		settings_bytes: QuotaLimit {
			default: 16 * KIB,
			max: 64 * KIB,
		},
		storage_bytes: QuotaLimit {
			default: MIB,
			max: 64 * MIB,
		},
		log_bytes: QuotaLimit {
			default: 0,
			max: 256 * MIB,
		},
		media_cache_bytes: QuotaLimit {
			default: 0,
			max: 512 * MIB,
		},
		network_response_bytes: QuotaLimit {
			default: MIB,
			max: 16 * MIB,
		},
		installed_package_bytes: 512 * MIB,
		installed_plugins: 128,
	};
}

/// Per-extension resource requests. Absent values use the host default for that category.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Quotas {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub settings_bytes: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub storage_bytes: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub log_bytes: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub media_cache_bytes: Option<u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub network_response_bytes: Option<u64>,
}

impl Quotas {
	pub fn is_empty(&self) -> bool {
		*self == Self::default()
	}
	fn each(&self, limits: &ResourceLimits) -> [(Option<u64>, QuotaLimit); 5] {
		[
			(self.settings_bytes, limits.settings_bytes),
			(self.storage_bytes, limits.storage_bytes),
			(self.log_bytes, limits.log_bytes),
			(self.media_cache_bytes, limits.media_cache_bytes),
			(self.network_response_bytes, limits.network_response_bytes),
		]
	}
	pub fn validate(&self, limits: &ResourceLimits) -> Result<(), Error> {
		if self
			.each(limits)
			.iter()
			.any(|(requested, limit)| requested.is_some_and(|value| value > limit.max))
		{
			return Err(Error::Limit);
		}
		Ok(())
	}
	/// The budget this extension actually gets in each category.
	pub fn effective(&self, limits: &ResourceLimits) -> EffectiveQuotas {
		let pick = |requested: Option<u64>, limit: QuotaLimit| {
			requested.unwrap_or(limit.default).min(limit.max)
		};
		EffectiveQuotas {
			settings_bytes: pick(self.settings_bytes, limits.settings_bytes),
			storage_bytes: pick(self.storage_bytes, limits.storage_bytes),
			log_bytes: pick(self.log_bytes, limits.log_bytes),
			media_cache_bytes: pick(self.media_cache_bytes, limits.media_cache_bytes),
			network_response_bytes: pick(
				self.network_response_bytes,
				limits.network_response_bytes,
			),
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveQuotas {
	pub settings_bytes: u64,
	pub storage_bytes: u64,
	pub log_bytes: u64,
	pub media_cache_bytes: u64,
	pub network_response_bytes: u64,
}

/// Host features this extension needs (`features`) or can use when present
/// (`optional_features`), each with a semantic-version requirement such as `"^1.0"`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Requirements {
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	pub features: BTreeMap<String, String>,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	pub optional_features: BTreeMap<String, String>,
}

impl Requirements {
	pub fn is_empty(&self) -> bool {
		self.features.is_empty() && self.optional_features.is_empty()
	}
}

/// Another extension this one works with. A required dependency must be installed and enabled
/// in the same scope with a version matching `version`; an optional one is used when present.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
	pub id: String,
	/// Semantic-version requirement, such as `"^1.2"` or `">=1.0, <3"`.
	pub version: String,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub optional: bool,
}

/// An extension that must not be enabled together with this one; `version` narrows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conflict {
	pub id: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub version: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingType {
	Toggle,
	Choice,
	Text,
	Number,
	/// Stored by the host in the OS credential store. Extension code never receives the value.
	Secret,
}

/// A setting value as stored and as delivered to the extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
	Bool(bool),
	Number(i64),
	Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingOption {
	pub value: String,
	pub label: String,
}

/// One field of the settings page the host builds for the extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingField {
	pub id: String,
	pub label: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub description: String,
	#[serde(rename = "type")]
	pub kind: SettingType,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub default: Option<SettingValue>,
	/// `choice` only.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub options: Vec<SettingOption>,
	/// `number` only.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub min: Option<i64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub max: Option<i64>,
	/// `text` only; defaults to 256.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub max_chars: Option<u32>,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub multiline: bool,
}

fn plain_text(value: &str, max: usize) -> bool {
	!value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

impl SettingField {
	fn validate(&self) -> Result<(), Error> {
		if !valid_id(&self.id)
			|| !plain_text(&self.label, 128)
			|| self.description.len() > 512
			|| self.description.chars().any(char::is_control)
		{
			return Err(Error::Invalid);
		}
		let type_only = |present: bool, kind: SettingType| present && self.kind != kind;
		if type_only(!self.options.is_empty(), SettingType::Choice)
			|| type_only(
				self.min.is_some() || self.max.is_some(),
				SettingType::Number,
			) || type_only(
			self.max_chars.is_some() || self.multiline,
			SettingType::Text,
		) {
			return Err(Error::Invalid);
		}
		match self.kind {
			SettingType::Secret if self.default.is_some() => return Err(Error::Invalid),
			SettingType::Choice => {
				let mut values = BTreeSet::new();
				if self.options.is_empty()
					|| self.options.len() > MAX_SETTING_OPTIONS
					|| self.options.iter().any(|option| {
						!plain_text(&option.value, 128)
							|| !plain_text(&option.label, 128)
							|| !values.insert(option.value.as_str())
					}) {
					return Err(Error::Invalid);
				}
			}
			SettingType::Number => {
				if let (Some(min), Some(max)) = (self.min, self.max)
					&& min > max
				{
					return Err(Error::Invalid);
				}
			}
			SettingType::Text => {
				if self
					.max_chars
					.is_some_and(|max| max == 0 || max > MAX_SETTING_TEXT_CHARS)
				{
					return Err(Error::Invalid);
				}
			}
			SettingType::Toggle | SettingType::Secret => {}
		}
		if let Some(default) = &self.default {
			self.check(default)?;
		}
		Ok(())
	}

	/// Whether `value` is acceptable for this field.
	pub fn check(&self, value: &SettingValue) -> Result<(), Error> {
		let ok = match (self.kind, value) {
			(SettingType::Toggle, SettingValue::Bool(_)) => true,
			(SettingType::Choice, SettingValue::Text(text)) => {
				self.options.iter().any(|option| option.value == *text)
			}
			(SettingType::Number, SettingValue::Number(number)) => {
				self.min.is_none_or(|min| *number >= min)
					&& self.max.is_none_or(|max| *number <= max)
			}
			(SettingType::Text, SettingValue::Text(text)) => {
				let max = self.max_chars.unwrap_or(256) as usize;
				text.chars().count() <= max
					&& !text
						.chars()
						.any(|c| c.is_control() && !(self.multiline && matches!(c, '\n' | '\t')))
			}
			_ => false,
		};
		ok.then_some(()).ok_or(Error::Invalid)
	}

	/// The value an extension sees before the user changes anything.
	pub fn initial(&self) -> Option<SettingValue> {
		self.default.clone().or(match self.kind {
			SettingType::Toggle => Some(SettingValue::Bool(false)),
			SettingType::Choice => self
				.options
				.first()
				.map(|option| SettingValue::Text(option.value.clone())),
			SettingType::Number => Some(SettingValue::Number(
				self.min
					.unwrap_or(0)
					.max(self.max.map_or(i64::MIN, |max| max.min(0))),
			)),
			SettingType::Text => Some(SettingValue::Text(String::new())),
			SettingType::Secret => None,
		})
	}
}

/// Saved values for the non-secret settings, with defaults filled in for absent ones.
/// Values that no longer fit the schema (after an update) fall back to the default.
pub fn resolve_settings(
	fields: &[SettingField],
	saved: &BTreeMap<String, SettingValue>,
) -> BTreeMap<String, SettingValue> {
	fields
		.iter()
		.filter(|field| field.kind != SettingType::Secret)
		.filter_map(|field| {
			let value = saved
				.get(&field.id)
				.filter(|value| field.check(value).is_ok())
				.cloned()
				.or_else(|| field.initial())?;
			Some((field.id.clone(), value))
		})
		.collect()
}

/// Checks values the user saved: only declared, non-secret fields, each valid, within budget.
pub fn validate_saved_settings(
	fields: &[SettingField],
	values: &BTreeMap<String, SettingValue>,
	budget: u64,
) -> Result<(), Error> {
	for (id, value) in values {
		let field = fields
			.iter()
			.find(|field| field.id == *id && field.kind != SettingType::Secret)
			.ok_or(Error::Invalid)?;
		field.check(value)?;
	}
	let bytes = serde_json::to_vec(values)
		.map_err(|_| Error::Invalid)?
		.len() as u64;
	if bytes > budget {
		return Err(Error::Limit);
	}
	Ok(())
}

/// Checks a secret before the host stores it.
pub fn validate_secret(fields: &[SettingField], id: &str, value: &str) -> Result<(), Error> {
	if !fields
		.iter()
		.any(|field| field.id == id && field.kind == SettingType::Secret)
	{
		return Err(Error::Invalid);
	}
	if value.is_empty() || value.len() > MAX_SECRET_CHARS || value.chars().any(char::is_control) {
		return Err(Error::Invalid);
	}
	Ok(())
}

fn version_req(text: &str) -> Result<semver::VersionReq, Error> {
	if text.is_empty() || text.len() > 64 {
		return Err(Error::Invalid);
	}
	semver::VersionReq::parse(text).map_err(|_| Error::Invalid)
}

fn feature_name(name: &str) -> bool {
	!name.is_empty()
		&& name.len() <= 64
		&& name
			.bytes()
			.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_'))
}

/// Validates the v2 fields of a manifest. `Manifest::validate` calls this.
pub(crate) fn validate_manifest(manifest: &Manifest) -> Result<(), Error> {
	let v2 = manifest.api_version >= API_V2;
	let uses_v2 = !manifest.requires.is_empty()
		|| !manifest.dependencies.is_empty()
		|| !manifest.conflicts.is_empty()
		|| !manifest.settings.is_empty()
		|| !manifest.quotas.is_empty()
		|| manifest
			.capabilities
			.iter()
			.any(|capability| V2_CAPABILITIES.contains(capability));
	if !v2 {
		return if uses_v2 { Err(Error::Version) } else { Ok(()) };
	}
	semver::Version::parse(&manifest.version).map_err(|_| Error::Invalid)?;
	let features = manifest
		.requires
		.features
		.iter()
		.chain(&manifest.requires.optional_features);
	if manifest.requires.features.len() + manifest.requires.optional_features.len()
		> MAX_FEATURE_REQUIREMENTS
	{
		return Err(Error::Limit);
	}
	for (name, requirement) in features {
		if !feature_name(name) {
			return Err(Error::Invalid);
		}
		version_req(requirement)?;
	}
	if manifest
		.requires
		.features
		.keys()
		.any(|name| manifest.requires.optional_features.contains_key(name))
	{
		return Err(Error::Invalid);
	}
	if manifest.dependencies.len() > MAX_DEPENDENCIES || manifest.conflicts.len() > MAX_DEPENDENCIES
	{
		return Err(Error::Limit);
	}
	let mut related = BTreeSet::new();
	for dependency in &manifest.dependencies {
		if !valid_id(&dependency.id)
			|| dependency.id == manifest.id
			|| !related.insert(dependency.id.as_str())
		{
			return Err(Error::Invalid);
		}
		version_req(&dependency.version)?;
	}
	for conflict in &manifest.conflicts {
		if !valid_id(&conflict.id)
			|| conflict.id == manifest.id
			|| !related.insert(conflict.id.as_str())
		{
			return Err(Error::Invalid);
		}
		if let Some(version) = &conflict.version {
			version_req(version)?;
		}
	}
	if manifest.settings.len() > MAX_SETTINGS
		|| manifest
			.settings
			.iter()
			.filter(|field| field.kind == SettingType::Secret)
			.count() > MAX_SECRETS
	{
		return Err(Error::Limit);
	}
	let mut ids = BTreeSet::new();
	for field in &manifest.settings {
		field.validate()?;
		if !ids.insert(field.id.as_str()) {
			return Err(Error::Invalid);
		}
	}
	manifest.quotas.validate(&ResourceLimits::DEFAULT)?;
	if manifest.kind == crate::ExtensionKind::Theme
		&& (!manifest.settings.is_empty()
			|| !manifest.dependencies.is_empty()
			|| !manifest.quotas.is_empty())
	{
		return Err(Error::Invalid);
	}
	Ok(())
}

/// Outcome of matching a manifest's feature requirements against this host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Negotiation {
	/// Optional features this host lacks or provides at an incompatible version.
	pub unavailable_optional: Vec<String>,
}

fn provided(name: &str) -> Option<semver::Version> {
	FEATURES
		.iter()
		.find(|(feature, _)| *feature == name)
		.and_then(|(_, version)| semver::Version::parse(version).ok())
}

/// Checks required features; a missing or incompatible one names the feature in the error.
pub fn negotiate(manifest: &Manifest) -> Result<Negotiation, String> {
	for (name, requirement) in &manifest.requires.features {
		let requirement = version_req(requirement).map_err(|error| error.to_string())?;
		match provided(name) {
			Some(version) if requirement.matches(&version) => {}
			Some(version) => {
				return Err(format!(
					"Requires host feature {name} {requirement}; this Harmony provides {version}"
				));
			}
			None => {
				return Err(format!(
					"Requires host feature {name}, which this Harmony does not provide"
				));
			}
		}
	}
	let unavailable_optional = manifest
		.requires
		.optional_features
		.iter()
		.filter(|(name, requirement)| {
			!version_req(requirement)
				.is_ok_and(|requirement| provided(name).is_some_and(|v| requirement.matches(&v)))
		})
		.map(|(name, _)| name.clone())
		.collect();
	Ok(Negotiation {
		unavailable_optional,
	})
}

/// An installed extension as seen by dependency checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledVersion<'a> {
	pub id: &'a str,
	pub version: &'a str,
	pub conflicts: &'a [Conflict],
}

fn version_matches(requirement: &str, version: &str) -> bool {
	match (version_req(requirement), semver::Version::parse(version)) {
		(Ok(requirement), Ok(version)) => requirement.matches(&version),
		_ => false,
	}
}

/// Checks that required dependencies are present at a compatible version and that nothing
/// installed conflicts with this manifest, in either direction. Returns a readable reason.
pub fn check_dependencies(
	manifest: &Manifest,
	installed: &[InstalledVersion],
) -> Result<(), String> {
	for dependency in manifest.dependencies.iter().filter(|d| !d.optional) {
		match installed.iter().find(|entry| entry.id == dependency.id) {
			Some(entry) if version_matches(&dependency.version, entry.version) => {}
			Some(entry) => {
				return Err(format!(
					"Needs {} {}; version {} is installed",
					dependency.id, dependency.version, entry.version
				));
			}
			None => {
				return Err(format!(
					"Needs the extension {} ({}); enable it first",
					dependency.id, dependency.version
				));
			}
		}
	}
	for entry in installed.iter().filter(|entry| entry.id != manifest.id) {
		let declared = manifest.conflicts.iter().any(|conflict| {
			conflict.id == entry.id
				&& conflict
					.version
					.as_deref()
					.is_none_or(|requirement| version_matches(requirement, entry.version))
		});
		let reverse = entry.conflicts.iter().any(|conflict| {
			conflict.id == manifest.id
				&& conflict
					.version
					.as_deref()
					.is_none_or(|requirement| version_matches(requirement, &manifest.version))
		});
		if declared || reverse {
			return Err(format!(
				"Conflicts with the installed extension {}",
				entry.id
			));
		}
	}
	Ok(())
}

/// One change to the extension's key-value store, applied after the output is validated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum KvOp {
	Set { key: String, value: String },
	Delete { key: String },
	Clear,
}

pub fn valid_kv_key(key: &str) -> bool {
	!key.is_empty() && key.len() <= MAX_KV_KEY_BYTES && !key.chars().any(char::is_control)
}

/// Something the extension asks the host to do before it is invoked again. The host answers
/// every request in the next invocation's `responses`, keyed by the request's `id`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostRequest {
	/// Values of the named keys; missing keys are `null`. Needs `storage`.
	KvGet { id: String, keys: Vec<String> },
	/// Keys starting with `prefix`, sorted, at most `limit`. Needs `storage`.
	KvList {
		id: String,
		#[serde(default)]
		prefix: String,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		limit: Option<u32>,
	},
	/// The start of an attachment on the message this action was chosen on.
	/// Needs `attachment_contents`.
	AttachmentRead {
		id: String,
		attachment_id: String,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		max_bytes: Option<u32>,
	},
	/// Public details of a server the account is in. Needs `server_profiles`.
	ServerProfile { id: String, guild_id: String },
}

impl HostRequest {
	pub fn id(&self) -> &str {
		match self {
			Self::KvGet { id, .. }
			| Self::KvList { id, .. }
			| Self::AttachmentRead { id, .. }
			| Self::ServerProfile { id, .. } => id,
		}
	}
	pub fn capability(&self) -> Capability {
		match self {
			Self::KvGet { .. } | Self::KvList { .. } => Capability::Storage,
			Self::AttachmentRead { .. } => Capability::AttachmentContents,
			Self::ServerProfile { .. } => Capability::ServerProfiles,
		}
	}
	/// Whether the extension worker can answer it from local files, without the app.
	pub fn local(&self) -> bool {
		matches!(self, Self::KvGet { .. } | Self::KvList { .. })
	}
	fn validate(&self) -> Result<(), Error> {
		if !valid_id(self.id()) {
			return Err(Error::Invalid);
		}
		match self {
			Self::KvGet { keys, .. } => {
				if keys.is_empty()
					|| keys.len() > MAX_KV_KEYS_PER_REQUEST
					|| !keys.iter().all(|key| valid_kv_key(key))
				{
					return Err(Error::Invalid);
				}
			}
			Self::KvList { prefix, limit, .. } => {
				if prefix.len() > MAX_KV_KEY_BYTES
					|| prefix.chars().any(char::is_control)
					|| limit.is_some_and(|limit| limit == 0 || limit > MAX_KV_LIST)
				{
					return Err(Error::Invalid);
				}
			}
			Self::AttachmentRead {
				attachment_id,
				max_bytes,
				..
			} => {
				crate::app::entity_id(attachment_id)?;
				if max_bytes.is_some_and(|max| max == 0 || max > MAX_ATTACHMENT_READ_BYTES) {
					return Err(Error::Invalid);
				}
			}
			Self::ServerProfile { guild_id, .. } => crate::app::entity_id(guild_id)?,
		}
		Ok(())
	}
}

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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct AttachmentContent {
	pub attachment_id: String,
	pub filename: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub content_type: Option<String>,
	/// The attachment's full size in bytes.
	pub size: u64,
	/// `text` when the bytes read are valid UTF-8, otherwise `base64`.
	pub encoding: ContentEncoding,
	pub data: String,
	/// Whether only the start of the attachment was read.
	pub truncated: bool,
}

/// Public server details from Discord, fetched by the host on request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
	pub roles: u32,
	pub emojis: u32,
	pub stickers: u32,
}

pub const MAX_SERVER_FEATURES: usize = 128;

impl ServerProfile {
	pub fn validate(&self) -> Result<(), Error> {
		crate::app::entity_id(&self.id)?;
		if let Some(owner) = &self.owner_id {
			crate::app::entity_id(owner)?;
		}
		let text = |value: &Option<String>, max: usize| {
			value
				.as_ref()
				.is_none_or(|value| value.len() <= max && !value.chars().any(char::is_control))
		};
		if self.name.is_empty()
			|| self.name.len() > 200
			|| !text(&self.vanity_url_code, 64)
			|| !text(&self.preferred_locale, 16)
			|| self
				.description
				.as_ref()
				.is_some_and(|description| description.len() > 2048)
			|| [&self.icon_url, &self.banner_url, &self.splash_url]
				.into_iter()
				.any(|url| url.as_deref().is_some_and(|url| !valid_https_url(url)))
			|| self.features.len() > MAX_SERVER_FEATURES
			|| self.features.iter().any(|feature| !plain_text(feature, 64))
		{
			return Err(Error::Invalid);
		}
		Ok(())
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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
}

/// The answer to one [`HostRequest`]: exactly one of `value` and `error`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostResponse {
	pub id: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub value: Option<ResponseValue>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<RequestError>,
}

impl HostResponse {
	pub fn ok(id: &str, value: ResponseValue) -> Self {
		Self {
			id: id.into(),
			value: Some(value),
			error: None,
		}
	}
	pub fn error(id: &str, code: RequestErrorCode, message: impl Into<String>) -> Self {
		Self {
			id: id.into(),
			value: None,
			error: Some(RequestError {
				code,
				message: message.into(),
			}),
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSource {
	Attachment,
	Embed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
	Image,
	Video,
	Audio,
	File,
}

/// A public link to media on the message an action was chosen on. Needs `media_links`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

impl MediaLink {
	fn validate(&self) -> Result<(), Error> {
		if !valid_https_url(&self.url)
			|| self
				.filename
				.as_ref()
				.is_some_and(|name| !plain_text(name, 1024))
			|| self
				.content_type
				.as_ref()
				.is_some_and(|kind| !plain_text(kind, 255))
		{
			return Err(Error::Invalid);
		}
		if let Some(id) = &self.attachment_id {
			crate::app::entity_id(id)?;
		}
		Ok(())
	}
}

/// Validates the v2 parts of an invocation the host is about to send.
pub(crate) fn validate_invocation(
	input: &crate::Invocation,
	manifest: &Manifest,
) -> Result<(), Error> {
	let v2 = manifest.api_version >= API_V2;
	let has_v2 = input.settings.is_some()
		|| input.secrets.is_some()
		|| input.media.is_some()
		|| !input.responses.is_empty()
		|| input.continuation.is_some()
		|| input.round != 0
		|| input.button.is_some();
	if has_v2 && !v2 {
		return Err(Error::Invalid);
	}
	if let Some(media) = &input.media {
		if !manifest.capabilities.contains(&Capability::MediaLinks) {
			return Err(Error::Capability);
		}
		if media.len() > MAX_MEDIA_LINKS {
			return Err(Error::Limit);
		}
		media.iter().try_for_each(MediaLink::validate)?;
	}
	if input.round > MAX_REQUEST_ROUNDS
		|| input.responses.len() > MAX_REQUESTS_PER_ROUND
		|| input
			.continuation
			.as_ref()
			.is_some_and(|text| text.len() > MAX_CONTINUATION_BYTES)
	{
		return Err(Error::Limit);
	}
	if let Some(secrets) = &input.secrets
		&& secrets.keys().any(|id| {
			!manifest
				.settings
				.iter()
				.any(|field| field.id == *id && field.kind == SettingType::Secret)
		}) {
		return Err(Error::Invalid);
	}
	if input
		.button
		.as_ref()
		.is_some_and(|button| !valid_id(button) || manifest.actions.iter().any(|a| a.id == *button))
	{
		return Err(Error::Invalid);
	}
	if let Some(settings) = &input.settings
		&& settings.iter().any(|(id, value)| {
			!manifest
				.settings
				.iter()
				.any(|field| field.id == *id && field.check(value).is_ok())
		}) {
		return Err(Error::Invalid);
	}
	Ok(())
}

/// Validates the v2 parts of an extension's output.
pub(crate) fn validate_output(output: &crate::Output, manifest: &Manifest) -> Result<(), Error> {
	let v2 = manifest.api_version >= API_V2;
	if !v2
		&& (!output.kv.is_empty() || !output.requests.is_empty() || output.continuation.is_some())
	{
		return Err(Error::Invalid);
	}
	if !output.kv.is_empty() {
		if !manifest.capabilities.contains(&Capability::Storage) {
			return Err(Error::Capability);
		}
		if output.kv.len() > MAX_KV_OPS {
			return Err(Error::Limit);
		}
		for op in &output.kv {
			match op {
				KvOp::Set { key, .. } | KvOp::Delete { key } if !valid_kv_key(key) => {
					return Err(Error::Invalid);
				}
				_ => {}
			}
		}
	}
	if output
		.continuation
		.as_ref()
		.is_some_and(|text| text.len() > MAX_CONTINUATION_BYTES)
	{
		return Err(Error::Limit);
	}
	if !output.requests.is_empty() {
		if output.requests.len() > MAX_REQUESTS_PER_ROUND {
			return Err(Error::Limit);
		}
		let mut ids = BTreeSet::new();
		for request in &output.requests {
			request.validate()?;
			if !ids.insert(request.id()) {
				return Err(Error::Invalid);
			}
			if !manifest.capabilities.contains(&request.capability()) {
				return Err(Error::Capability);
			}
		}
		// A round that asks for more information shows nothing yet; the final round does.
		if output.replacement.is_some()
			|| !output.panel.is_empty()
			|| !output.effects.is_empty()
			|| output.appearance.is_some()
			|| output.rich_presence.is_some()
			|| output.api_proxy.is_some()
			|| output.interface_tweaks.is_some()
		{
			return Err(Error::Invalid);
		}
	}
	Ok(())
}

/// Every surface this host can invoke.
pub const SURFACES: &[crate::Surface] = &[
	crate::Surface::Message,
	crate::Surface::Composer,
	crate::Surface::Panel,
	crate::Surface::Activation,
	crate::Surface::MessageEvent,
	crate::Surface::AppEvent,
	crate::Surface::Tick,
	crate::Surface::User,
	crate::Surface::Channel,
	crate::Surface::Server,
	crate::Surface::Role,
];

/// Host request kinds this host answers.
pub const REQUEST_KINDS: &[&str] = &["kv_get", "kv_list", "attachment_read", "server_profile"];

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{ExtensionKind, Output};

	fn manifest(json: &str) -> Manifest {
		serde_json::from_str(json).unwrap()
	}
	const BASE: &str = r#""id":"probe","name":"Probe","version":"1.2.0","author":"Synthetic",
		"license":"MIT","source":"https://example.org/probe","kind":"plugin",
		"actions":[{"id":"open","label":"Open","surface":"panel"}]"#;

	#[test]
	fn v1_manifests_cannot_reach_v2_fields_and_existing_v1_packages_still_validate() {
		let v1 = manifest(&format!(
			r#"{{"api_version":1,{BASE},"capabilities":["storage"]}}"#
		));
		v1.validate().unwrap();
		for extra in [
			r#""settings":[{"id":"x","label":"X","type":"toggle"}]"#,
			r#""capabilities":["server_profiles"]"#,
			r#""requires":{"features":{"settings":"^1"}}"#,
		] {
			let upgraded = manifest(&format!(r#"{{"api_version":1,{BASE},{extra}}}"#));
			assert!(
				matches!(upgraded.validate(), Err(Error::Version)),
				"{extra}"
			);
			let v2 = manifest(&format!(r#"{{"api_version":2,{BASE},{extra}}}"#));
			assert!(v2.validate().is_ok(), "{extra}");
		}
		// A secret can never ship a default value in the package.
		let secret = manifest(&format!(
			r#"{{"api_version":2,{BASE},"settings":[{{"id":"key","label":"Key","type":"secret","default":"x"}}]}}"#
		));
		assert!(secret.validate().is_err());
	}

	#[test]
	fn negotiation_rejects_missing_or_incompatible_required_features_only() {
		let probe = |requires: &str| {
			manifest(&format!(
				r#"{{"api_version":2,{BASE},"requires":{requires}}}"#
			))
		};
		assert!(negotiate(&probe(r#"{"features":{"settings":"^1.0"}}"#)).is_ok());
		assert!(negotiate(&probe(r#"{"features":{"settings":"^2"}}"#)).is_err());
		assert!(negotiate(&probe(r#"{"features":{"future.thing":"^1"}}"#)).is_err());
		let negotiation = negotiate(&probe(
			r#"{"optional_features":{"settings":"^1","future.thing":"^1","secrets":">=9"}}"#,
		))
		.unwrap();
		assert_eq!(
			negotiation.unavailable_optional,
			["future.thing", "secrets"]
		);
	}

	#[test]
	fn dependencies_need_matching_versions_and_conflicts_apply_both_ways() {
		let dependent = manifest(&format!(
			r#"{{"api_version":2,{BASE},"dependencies":[{{"id":"lib","version":"^1.4"}},
			{{"id":"extra","version":"^1","optional":true}}],"conflicts":[{{"id":"rival","version":"<2"}}]}}"#
		));
		assert!(dependent.validate().is_ok());
		let lib = |version| InstalledVersion {
			id: "lib",
			version,
			conflicts: &[],
		};
		assert!(check_dependencies(&dependent, &[]).is_err());
		assert!(check_dependencies(&dependent, &[lib("1.3.9")]).is_err());
		assert!(check_dependencies(&dependent, &[lib("1.5.0")]).is_ok());
		let rival = |version| InstalledVersion {
			id: "rival",
			version,
			conflicts: &[],
		};
		assert!(check_dependencies(&dependent, &[lib("1.5.0"), rival("1.0.0")]).is_err());
		assert!(check_dependencies(&dependent, &[lib("1.5.0"), rival("2.0.0")]).is_ok());
		// The other extension's declared conflict also blocks this one.
		let blocks = [Conflict {
			id: "probe".into(),
			version: None,
		}];
		let blocker = InstalledVersion {
			id: "blocker",
			version: "1.0.0",
			conflicts: &blocks,
		};
		assert!(check_dependencies(&dependent, &[lib("1.5.0"), blocker]).is_err());
	}

	#[test]
	fn settings_resolve_defaults_reject_stale_values_and_keep_secrets_out() {
		let probe = manifest(&format!(
			r#"{{"api_version":2,{BASE},"settings":[
			{{"id":"mode","label":"Mode","type":"choice","options":[{{"value":"a","label":"A"}},{{"value":"b","label":"B"}}]}},
			{{"id":"count","label":"Count","type":"number","min":1,"max":5,"default":3}},
			{{"id":"key","label":"Key","type":"secret"}}]}}"#
		));
		assert!(probe.validate().is_ok());
		let saved = BTreeMap::from([
			(
				"mode".to_owned(),
				SettingValue::Text("removed-option".into()),
			),
			("count".to_owned(), SettingValue::Number(5)),
			("key".to_owned(), SettingValue::Text("leaked".into())),
		]);
		let resolved = resolve_settings(&probe.settings, &saved);
		assert_eq!(resolved.get("mode"), Some(&SettingValue::Text("a".into())));
		assert_eq!(resolved.get("count"), Some(&SettingValue::Number(5)));
		assert!(!resolved.contains_key("key"));
		let budget = ResourceLimits::DEFAULT.settings_bytes.default;
		assert!(validate_saved_settings(&probe.settings, &saved, budget).is_err());
		let out_of_range = BTreeMap::from([("count".to_owned(), SettingValue::Number(6))]);
		assert!(validate_saved_settings(&probe.settings, &out_of_range, budget).is_err());
		assert!(validate_secret(&probe.settings, "key", "value").is_ok());
		assert!(validate_secret(&probe.settings, "mode", "value").is_err());
		assert!(validate_secret(&probe.settings, "key", "line\nbreak").is_err());
	}

	#[test]
	fn request_rounds_need_grants_unique_ids_and_cannot_show_results_yet() {
		let mut probe = manifest(&format!(
			r#"{{"api_version":2,{BASE},"capabilities":["storage"]}}"#
		));
		let kv = HostRequest::KvGet {
			id: "a".into(),
			keys: vec!["k".into()],
		};
		let output = |requests: Vec<HostRequest>| Output {
			requests,
			continuation: Some("state".into()),
			..Default::default()
		};
		assert!(validate_output(&output(vec![kv.clone()]), &probe).is_ok());
		assert!(matches!(
			validate_output(&output(vec![kv.clone(), kv.clone()]), &probe),
			Err(Error::Invalid)
		));
		let lookup = HostRequest::ServerProfile {
			id: "b".into(),
			guild_id: "10".into(),
		};
		assert!(matches!(
			validate_output(&output(vec![lookup]), &probe),
			Err(Error::Capability)
		));
		let mut showing = output(vec![kv.clone()]);
		showing.replacement = Some("too early".into());
		assert!(validate_output(&showing, &probe).is_err());
		probe.api_version = crate::API_VERSION;
		assert!(validate_output(&output(vec![kv]), &probe).is_err());
		assert_eq!(probe.kind, ExtensionKind::Plugin);
	}

	#[test]
	fn v2_catalog_is_separate_from_the_one_every_release_reads() {
		let catalog = |api: u32, entry_api: u32| {
			format!(
				r#"{{"api_version":{api},"entries":[{{"manifest":{{"api_version":{entry_api},{BASE}}},
				"release_url":"https://example.org/p","sha256":"{}","download_bytes":10,
				"source_commit":"{}"}}]}}"#,
				"a".repeat(64),
				"b".repeat(40)
			)
		};
		// Releases before API v2 parse only `catalog.json` and refuse anything else.
		assert!(matches!(
			crate::parse_catalog(catalog(2, 2).as_bytes()),
			Err(Error::Version)
		));
		assert!(crate::parse_catalog_v2(catalog(2, 1).as_bytes()).is_err());
		let v1 = crate::parse_catalog(catalog(1, 1).as_bytes()).unwrap();
		let v2 = crate::parse_catalog_v2(catalog(2, 2).as_bytes()).unwrap();
		let merged = crate::merge_catalogs(Some(v1.clone()), Some(v2)).unwrap();
		assert_eq!(merged.entries.len(), 1);
		assert_eq!(merged.entries[0].manifest.api_version, API_V2);
		let only_v1 = crate::merge_catalogs(Some(v1), None).unwrap();
		assert_eq!(only_v1.entries[0].manifest.api_version, 1);
	}

	#[test]
	fn published_v2_catalog_parses_with_host_rules() {
		let catalog =
			crate::parse_catalog_v2(include_bytes!("../../../extensions/catalog-v2.json")).unwrap();
		assert!(
			catalog
				.entries
				.iter()
				.all(|entry| entry.manifest.api_version == API_V2)
		);
	}
}
