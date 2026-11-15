//! `GET /guilds/{id}?with_counts=true` for members (documented guild object), reduced to the
//! public details extensions may look up. Lists are counted, never kept.
use crate::DecodeError;
use model::{
	Id,
	server_profile::{MAX_SERVER_FEATURES, ServerProfile},
};
use serde::{Deserialize, de::IgnoredAny};

pub const MAX_PROFILE_WIRE: usize = 2 * 1024 * 1024;

#[derive(Deserialize)]
struct Guild {
	id: Id,
	name: String,
	#[serde(default)]
	description: Option<String>,
	#[serde(default)]
	icon: Option<String>,
	#[serde(default)]
	banner: Option<String>,
	#[serde(default)]
	splash: Option<String>,
	#[serde(default)]
	owner_id: Option<Id>,
	#[serde(default)]
	verification_level: Option<u8>,
	#[serde(default)]
	explicit_content_filter: Option<u8>,
	#[serde(default)]
	nsfw_level: Option<u8>,
	#[serde(default)]
	mfa_level: Option<u8>,
	#[serde(default)]
	premium_tier: Option<u8>,
	#[serde(default)]
	premium_subscription_count: Option<u64>,
	#[serde(default)]
	approximate_member_count: Option<u64>,
	#[serde(default)]
	approximate_presence_count: Option<u64>,
	#[serde(default)]
	max_members: Option<u64>,
	#[serde(default)]
	vanity_url_code: Option<String>,
	#[serde(default)]
	preferred_locale: Option<String>,
	#[serde(default)]
	features: Vec<String>,
	#[serde(default)]
	roles: Vec<IgnoredAny>,
	#[serde(default)]
	emojis: Vec<IgnoredAny>,
	#[serde(default)]
	stickers: Vec<IgnoredAny>,
}

fn image_hash(hash: Option<String>) -> Option<String> {
	hash.filter(|hash| {
		!hash.is_empty()
			&& hash.len() <= 64
			&& hash.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
	})
}

fn short_text(text: Option<String>, max: usize) -> Option<String> {
	text.filter(|text| !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control))
}

pub fn decode_server_profile(guild: Id, bytes: &[u8]) -> Result<ServerProfile, DecodeError> {
	if bytes.len() > MAX_PROFILE_WIRE {
		return Err(DecodeError);
	}
	let wire: Guild = crate::decode_limited(bytes, MAX_PROFILE_WIRE)?;
	if wire.id != guild || wire.name.is_empty() || wire.name.len() > 200 {
		return Err(DecodeError);
	}
	let mut features: Vec<String> = wire
		.features
		.into_iter()
		.filter(|feature| {
			!feature.is_empty() && feature.len() <= 64 && !feature.chars().any(char::is_control)
		})
		.take(MAX_SERVER_FEATURES)
		.collect();
	features.sort();
	features.dedup();
	Ok(ServerProfile {
		id: wire.id,
		name: wire.name,
		description: wire
			.description
			.filter(|text| text.len() <= 2048 && !text.is_empty()),
		icon: image_hash(wire.icon),
		banner: image_hash(wire.banner),
		splash: image_hash(wire.splash),
		owner_id: wire.owner_id,
		verification_level: wire.verification_level,
		explicit_content_filter: wire.explicit_content_filter,
		nsfw_level: wire.nsfw_level,
		mfa_level: wire.mfa_level,
		premium_tier: wire.premium_tier,
		premium_subscription_count: wire.premium_subscription_count,
		approximate_member_count: wire.approximate_member_count,
		approximate_presence_count: wire.approximate_presence_count,
		max_members: wire.max_members,
		vanity_url_code: short_text(wire.vanity_url_code, 64),
		preferred_locale: short_text(wire.preferred_locale, 16),
		features,
		roles: wire.roles.len().min(u32::MAX as usize) as u32,
		emojis: wire.emojis.len().min(u32::MAX as usize) as u32,
		stickers: wire.stickers.len().min(u32::MAX as usize) as u32,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn profiles_count_lists_reject_other_servers_and_drop_bad_image_hashes() {
		let body =
			br#"{"id":"10","name":"Synthetic","owner_id":"20","icon":"a_abc","banner":"../x",
			"premium_tier":2,"approximate_member_count":42,"features":["COMMUNITY","COMMUNITY","\u0007"],
			"roles":[{},{}],"emojis":[{}],"stickers":[],"unknown":{"nested":true}}"#;
		let profile = decode_server_profile(Id(10), body).unwrap();
		assert_eq!((profile.roles, profile.emojis, profile.stickers), (2, 1, 0));
		assert_eq!(profile.icon.as_deref(), Some("a_abc"));
		assert_eq!(profile.banner, None);
		assert_eq!(profile.features, ["COMMUNITY"]);
		assert_eq!(profile.owner_id, Some(Id(20)));
		assert!(decode_server_profile(Id(11), body).is_err());
	}
}
