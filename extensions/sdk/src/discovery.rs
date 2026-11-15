use serde::{Deserialize, Serialize};
/// Nonprivate host support metadata. Absent on hosts predating discovery.
/// Supported capability names do not imply user consent. Unknown future names are retained.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
	pub api_version: u32,
	pub sdk_revision: u32,
	pub capabilities: Vec<String>,
	pub app_events: Vec<String>,
	/// Interface tweak names this host honors; empty on hosts predating tweaks.
	#[serde(default)]
	pub interface_tweaks: Vec<String>,
	/// Manifest levels the host installs; empty on hosts predating API v2.
	#[serde(default)]
	pub api_versions: Vec<u32>,
	/// Host features and the semantic version provided, such as `"storage.kv": "1.0.0"`.
	#[serde(default)]
	pub features: std::collections::BTreeMap<String, String>,
	#[serde(default)]
	pub surfaces: Vec<String>,
	/// UI contribution slots the host renders.
	#[serde(default)]
	pub slots: Vec<String>,
	/// `HostRequest` kinds the host answers.
	#[serde(default)]
	pub request_kinds: Vec<String>,
	/// Resource budgets; `null` on hosts predating API v2.
	#[serde(default)]
	pub limits: Option<serde_json::Value>,
}
impl HostInfo {
	pub fn supports(&self, capability: &str) -> bool {
		self.capabilities.iter().any(|name| name == capability)
	}
	pub fn supports_event(&self, event: &str) -> bool {
		self.app_events.iter().any(|name| name == event)
	}
	/// Whether the host provides `feature` at major version `major` and at least `minor`.
	/// Features follow semantic versioning, so this is the usual compatibility check.
	pub fn supports_feature(&self, feature: &str, major: u64, minor: u64) -> bool {
		self.features.get(feature).is_some_and(|version| {
			let mut parts = version.split('.').map(|part| part.parse::<u64>());
			matches!(
				(parts.next(), parts.next()),
				(Some(Ok(found_major)), Some(Ok(found_minor)))
					if found_major == major && found_minor >= minor
			)
		})
	}
	pub fn supports_request(&self, kind: &str) -> bool {
		self.request_kinds.iter().any(|name| name == kind)
	}
	/// Whether the host honors the named interface tweak, such as `"expanded_roles"`.
	pub fn supports_tweak(&self, tweak: &str) -> bool {
		self.interface_tweaks.iter().any(|name| name == tweak)
	}
}
