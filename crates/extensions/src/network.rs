//! API v2 network requests made by Harmony for an extension.
//!
//! An extension lists the sites it may contact in `origins`: fixed host names, or
//! `setting:<id>` for the host of a URL the user typed into that text setting. The user sees
//! the list before enabling. Harmony makes each request itself: HTTPS on port 443 only, no
//! private networks, no redirects followed, a few safe headers, and secrets added by name so
//! the extension never sees them. Answers are bounded by the network response quota.
use crate::{Capability, Error, Manifest, SettingType};
use serde::{Deserialize, Serialize};

pub const MAX_ORIGINS: usize = 8;
pub const MAX_REQUEST_HEADERS: usize = 12;
pub const MAX_REQUEST_BODY: usize = 64 * 1024;
pub const MAX_URL: usize = 4096;
/// One extension may make at most this many requests per minute.
pub const REQUESTS_PER_MINUTE: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
	Get,
	Post,
	Put,
	Patch,
	Delete,
}

/// A header whose value Harmony fills from one of the extension's secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretHeader {
	pub header: String,
	/// A declared `secret` setting.
	pub secret: String,
	/// Text put before the secret, such as `"DeepL-Auth-Key "` or `"Bearer "`.
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub prefix: String,
}

/// A query parameter whose value Harmony fills from one of the extension's secrets, for
/// services that take their API key in the address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretParam {
	pub param: String,
	/// A declared `secret` setting.
	pub secret: String,
}

/// The parts of an `http` request after its `id`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpRequest {
	pub method: HttpMethod,
	pub url: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub headers: Vec<(String, String)>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub secret_headers: Vec<SecretHeader>,
	/// Query parameters appended by Harmony with secret values.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub secret_params: Vec<SecretParam>,
	/// Text body; send binary data base64-encoded with `body_base64`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub body: Option<String>,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub body_base64: bool,
	/// Largest answer wanted, in bytes, within the network response quota.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub max_bytes: Option<u32>,
}

/// What came back. The body is text when it is valid UTF-8, otherwise base64.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpResponse {
	pub status: u16,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub content_type: Option<String>,
	/// For a redirect, where it pointed; Harmony does not follow redirects.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub location: Option<String>,
	pub encoding: crate::ContentEncoding,
	pub body: String,
	pub truncated: bool,
}

/// Headers an extension may not set: Harmony owns them, or they carry credentials.
const RESERVED_HEADERS: &[&str] = &[
	"host",
	"cookie",
	"authorization",
	"proxy-authorization",
	"content-length",
	"transfer-encoding",
	"connection",
	"upgrade",
	"te",
	"x-super-properties",
	"x-discord-locale",
	"x-fingerprint",
];

fn header_name(name: &str) -> bool {
	!name.is_empty()
		&& name.len() <= 64
		&& name
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn header_value(value: &str) -> bool {
	value.len() <= 1024 && !value.chars().any(char::is_control)
}

/// A host name as it may appear in `origins`: lowercase letters, digits, hyphens and dots,
/// with at least one dot, and no address literals.
pub fn valid_origin_host(host: &str) -> bool {
	host.len() <= 253
		&& host.contains('.')
		&& !host.starts_with('.')
		&& !host.ends_with('.')
		&& host
			.bytes()
			.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
		&& !host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// Manifest rules: origins need `network`; `setting:` origins name a text setting.
pub(crate) fn validate_manifest(manifest: &Manifest) -> Result<(), Error> {
	if manifest.origins.is_empty() {
		if manifest.capabilities.contains(&Capability::Network) {
			return Err(Error::Invalid);
		}
		return Ok(());
	}
	if !manifest.capabilities.contains(&Capability::Network) {
		return Err(Error::Capability);
	}
	if manifest.origins.len() > MAX_ORIGINS {
		return Err(Error::Limit);
	}
	for origin in &manifest.origins {
		let ok = match origin.strip_prefix("setting:") {
			Some(id) => manifest
				.settings
				.iter()
				.any(|field| field.id == id && field.kind == SettingType::Text),
			None => valid_origin_host(origin),
		};
		if !ok {
			return Err(Error::Invalid);
		}
	}
	Ok(())
}

impl HttpRequest {
	/// Shape checks the host makes before any network access; where the request may go is
	/// checked by the host against the origins and the user's settings.
	pub fn validate(&self, manifest: &Manifest) -> Result<(), Error> {
		if self.url.len() > MAX_URL
			|| !self.url.starts_with("https://")
			|| self.headers.len() + self.secret_headers.len() + self.secret_params.len()
				> MAX_REQUEST_HEADERS
			|| self
				.body
				.as_ref()
				.is_some_and(|body| body.len() > MAX_REQUEST_BODY)
			|| self.body_base64 && self.body.is_none()
			|| self.max_bytes == Some(0)
		{
			return Err(Error::Invalid);
		}
		for (name, value) in &self.headers {
			if !header_name(name)
				|| !header_value(value)
				|| RESERVED_HEADERS.contains(&name.to_ascii_lowercase().as_str())
			{
				return Err(Error::Invalid);
			}
		}
		let declared = |secret: &str| {
			manifest
				.settings
				.iter()
				.any(|field| field.id == secret && field.kind == SettingType::Secret)
		};
		for secret in &self.secret_headers {
			if !header_name(&secret.header)
				|| !header_value(&secret.prefix)
				|| !declared(&secret.secret)
			{
				return Err(Error::Invalid);
			}
		}
		for secret in &self.secret_params {
			if !header_name(&secret.param) || !declared(&secret.secret) {
				return Err(Error::Invalid);
			}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn manifest(capabilities: &str, origins: &str) -> Manifest {
		serde_json::from_str(&format!(
			r#"{{"api_version":2,"id":"probe","name":"Probe","version":"1.0.0","author":"Synthetic",
			"license":"MIT","source":"https://example.org/probe","kind":"plugin",
			"capabilities":[{capabilities}],"origins":[{origins}],
			"settings":[{{"id":"server","label":"Server","type":"text"}},{{"id":"key","label":"Key","type":"secret"}}],
			"actions":[{{"id":"open","label":"Open","surface":"panel"}}]}}"#
		))
		.unwrap()
	}

	#[test]
	fn origins_and_requests_follow_the_network_rules() {
		assert!(
			manifest(r#""network""#, r#""api.example.org","setting:server""#)
				.validate()
				.is_ok()
		);
		for (capabilities, origins) in [
			("", r#""api.example.org""#),
			(r#""network""#, ""),
			(r#""network""#, r#""127.0.0.1""#),
			(r#""network""#, r#""localhost""#),
			(r#""network""#, r#""API.example.org""#),
			(r#""network""#, r#""setting:key""#),
			(r#""network""#, r#""setting:missing""#),
		] {
			assert!(
				manifest(capabilities, origins).validate().is_err(),
				"{capabilities} {origins}"
			);
		}
		let granted = manifest(r#""network""#, r#""api.example.org""#);
		let request = |headers: Vec<(&str, &str)>, secret: Option<&str>| HttpRequest {
			method: HttpMethod::Post,
			url: "https://api.example.org/v1".into(),
			headers: headers
				.into_iter()
				.map(|(name, value)| (name.into(), value.into()))
				.collect(),
			secret_headers: secret
				.map(|secret| SecretHeader {
					header: "Authorization".into(),
					secret: secret.into(),
					prefix: "Bearer ".into(),
				})
				.into_iter()
				.collect(),
			secret_params: Vec::new(),
			body: Some("{}".into()),
			body_base64: false,
			max_bytes: None,
		};
		assert!(
			request(vec![("Content-Type", "application/json")], Some("key"))
				.validate(&granted)
				.is_ok()
		);
		// Credentials only come from secrets, and only from declared secret settings.
		assert!(
			request(vec![("Authorization", "Bearer leaked")], None)
				.validate(&granted)
				.is_err()
		);
		assert!(
			request(vec![("Cookie", "a=b")], None)
				.validate(&granted)
				.is_err()
		);
		assert!(request(vec![], Some("server")).validate(&granted).is_err());
		assert!(
			request(vec![("X-Ok", "line\nbreak")], None)
				.validate(&granted)
				.is_err()
		);
	}
}
