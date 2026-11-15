//! Public server details fetched on demand, for extensions that look a server up.
use crate::Id;

/// Fields are copied from Discord's guild object; counts of roles, emojis and stickers only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerProfile {
	pub id: Id,
	pub name: String,
	pub description: Option<String>,
	pub icon: Option<String>,
	pub banner: Option<String>,
	pub splash: Option<String>,
	pub owner_id: Option<Id>,
	pub verification_level: Option<u8>,
	pub explicit_content_filter: Option<u8>,
	pub nsfw_level: Option<u8>,
	pub mfa_level: Option<u8>,
	pub premium_tier: Option<u8>,
	pub premium_subscription_count: Option<u64>,
	pub approximate_member_count: Option<u64>,
	pub approximate_presence_count: Option<u64>,
	pub max_members: Option<u64>,
	pub vanity_url_code: Option<String>,
	pub preferred_locale: Option<String>,
	pub features: Vec<String>,
	pub roles: u32,
	pub emojis: u32,
	pub stickers: u32,
}

/// Most entries kept from a server's feature list.
pub const MAX_SERVER_FEATURES: usize = 128;
