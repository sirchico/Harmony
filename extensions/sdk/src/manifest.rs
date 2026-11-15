use serde::{Deserialize, Serialize};

/// Package metadata for authoring tools and tests, not an invocation or a permission grant.
/// Serde checks the document shape; the host remains responsible for semantic validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
	pub api_version: u32,
	pub id: String,
	pub name: String,
	pub version: String,
	pub author: String,
	pub license: String,
	pub source: String,
	pub kind: ExtensionKind,
	#[serde(default)]
	pub capabilities: Vec<Capability>,
	#[serde(default)]
	pub actions: Vec<Action>,
	/// API v2: host features required (`features`) or used when present
	/// (`optional_features`), each with a version requirement such as `"^1.0"`.
	#[serde(default, skip_serializing_if = "Requirements::is_empty")]
	pub requires: Requirements,
	/// API v2: other extensions this one needs or can use.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub dependencies: Vec<Dependency>,
	/// API v2: extensions that must not be enabled together with this one.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub conflicts: Vec<Conflict>,
	/// API v2: the settings page the host builds, saves and passes to every invocation.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub settings: Vec<SettingField>,
	/// API v2: resource budgets beyond the host defaults.
	#[serde(default, skip_serializing_if = "Quotas::is_empty")]
	pub quotas: Quotas,
	/// API v2: interface slots this extension adds items to; needs `ui_contributions`.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub slots: Vec<crate::Slot>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Requirements {
	#[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
	pub features: std::collections::BTreeMap<String, String>,
	#[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
	pub optional_features: std::collections::BTreeMap<String, String>,
}
impl Requirements {
	pub fn is_empty(&self) -> bool {
		self.features.is_empty() && self.optional_features.is_empty()
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
	pub id: String,
	/// Semantic-version requirement, such as `"^1.2"`.
	pub version: String,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub optional: bool,
}

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
	/// Kept in the OS credential store by the host; extension code never sees the value.
	Secret,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingOption {
	pub value: String,
	pub label: String,
}

/// One field of the host-built settings page.
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
	pub default: Option<crate::SettingValue>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub options: Vec<SettingOption>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub min: Option<i64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub max: Option<i64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub max_chars: Option<u32>,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub multiline: bool,
}

/// Resource budgets in bytes; absent values use the host defaults in `HostInfo::limits`.
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionKind {
	Plugin,
	Theme,
}

/// Explicit permissions requested by a manifest. Each still requires user consent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
	RichPresence,
	RelationshipControl,
	AccountControl,
	AudioSettings,
	VoiceConnect,
	CameraControl,

	MessageSend,
	MessageManage,
	ReactionsControl,
	ReadStateControl,
	ThreadsControl,
	ChannelControl,
	ServerControl,
	RoleControl,
	ModerationControl,
	MediaControl,
	ActionFeedback,
	DataQueries,
	MessagingSettings,
	GuildFolders,

	MessageContent,
	ForumData,
	ConversationActivity,
	ChannelMetadata,
	MemberDetails,
	SelectedMessage,
	Composer,
	Storage,
	DeletedMessages,
	ImageSharing,
	Appearance,
	MessageEvents,
	AppContext,
	ChannelDirectory,
	Timeline,
	Members,
	Presence,
	VoiceState,
	ReadState,
	LocalSettings,
	NotificationSettings,
	Navigation,
	LocalNotices,
	ClipboardWrite,
	VoiceControl,
	AppEvents,
	AccountProfile,
	GuildDirectory,
	ChannelDetails,
	DataEvents,
	MessageDetails,
	Relationships,
	ExternalNavigation,
	ContextMenus,
	InterfaceTweaks,
	/// API v2: links to media on the message a `message` action was chosen on.
	MediaLinks,
	/// API v2: read the start of an attachment on the chosen message.
	AttachmentContents,
	/// API v2: look up public details of servers the account is in.
	ServerProfiles,
	/// API v2: add items Harmony draws in the declared `slots`.
	UiContributions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
	Message,
	Composer,
	Panel,
	Activation,
	MessageEvent,
	AppEvent,
	Tick,
	User,
	Channel,
	Server,
	Role,
	/// API v2: an icon button in a message's hover toolbar; receives the message.
	MessageToolbar,
	/// API v2: an image context-menu entry (a submenu with `choices`); receives the image as
	/// the only `media` link and the chosen entry as `button`.
	Image,
	/// API v2: an icon button on file attachments matching `accepts`; receives the file as
	/// the only `media` link. A result that only copies text is copied at once.
	Attachment,
}

/// Declared entry point; its ID arrives in `Invocation::action`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
	pub id: String,
	pub label: String,
	pub surface: Surface,
	/// API v2: the icon drawn for toolbar, image and attachment actions.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub icon: Option<crate::ContributionIcon>,
	/// API v2, `image` only: submenu entries (at most 12).
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub choices: Vec<ActionChoice>,
	/// API v2, `attachment` only (required): which files show the button.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub accepts: Option<AttachmentFilter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionChoice {
	pub id: String,
	pub label: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub icon: Option<crate::ContributionIcon>,
}

/// Files match by extension (lowercase, no dot) or content type prefix, up to `max_bytes`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AttachmentFilter {
	pub extensions: Vec<String>,
	pub content_types: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub max_bytes: Option<u64>,
}
