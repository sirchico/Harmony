use crate::{AppEventKind, Capability, InterfaceTweak, ResourceLimits, Surface};
use serde::Serialize;
use std::collections::BTreeMap;
/// Public host contract metadata; contains no account or installation information.
///
/// Every field is additive: extensions ignore names they do not know, and a missing field
/// means the host predates it.
#[derive(Serialize)]
pub struct HostInfo {
	/// The Wasm buffer and JSON contract version. Unchanged since API v1.
	pub api_version: u32,
	pub sdk_revision: u32,
	pub capabilities: &'static [Capability],
	pub app_events: &'static [AppEventKind],
	pub interface_tweaks: &'static [InterfaceTweak],
	/// Manifest levels this host installs, oldest first.
	pub api_versions: &'static [u32],
	/// Host features and the semantic version provided, for runtime negotiation.
	pub features: BTreeMap<&'static str, &'static str>,
	/// Surfaces an action may declare.
	pub surfaces: &'static [Surface],
	/// UI contribution slots. Empty until the contribution API is available.
	pub slots: &'static [&'static str],
	/// `HostRequest` kinds this host answers.
	pub request_kinds: &'static [&'static str],
	/// Resource budgets: per-category defaults and the most a manifest may request.
	pub limits: ResourceLimits,
}
impl HostInfo {
	pub fn current() -> Self {
		Self {
			api_version: crate::API_VERSION,
			sdk_revision: 2,
			capabilities: &[
				Capability::ApiProxy,
				Capability::RichPresence,
				Capability::ActionFeedback,
				Capability::DataQueries,
				Capability::MessagingSettings,
				Capability::GuildFolders,
				Capability::RelationshipControl,
				Capability::AccountControl,
				Capability::AudioSettings,
				Capability::VoiceConnect,
				Capability::CameraControl,
				Capability::MessageSend,
				Capability::MessageManage,
				Capability::ReactionsControl,
				Capability::ReadStateControl,
				Capability::ThreadsControl,
				Capability::ChannelControl,
				Capability::ServerControl,
				Capability::RoleControl,
				Capability::ModerationControl,
				Capability::MediaControl,
				Capability::MessageContent,
				Capability::ForumData,
				Capability::ConversationActivity,
				Capability::ChannelMetadata,
				Capability::MemberDetails,
				Capability::SelectedMessage,
				Capability::Composer,
				Capability::Storage,
				Capability::DeletedMessages,
				Capability::ImageSharing,
				Capability::Appearance,
				Capability::MessageEvents,
				Capability::AppContext,
				Capability::ChannelDirectory,
				Capability::Timeline,
				Capability::Members,
				Capability::Presence,
				Capability::VoiceState,
				Capability::ReadState,
				Capability::LocalSettings,
				Capability::NotificationSettings,
				Capability::Navigation,
				Capability::LocalNotices,
				Capability::ClipboardWrite,
				Capability::VoiceControl,
				Capability::AppEvents,
				Capability::AccountProfile,
				Capability::GuildDirectory,
				Capability::ChannelDetails,
				Capability::DataEvents,
				Capability::MessageDetails,
				Capability::Relationships,
				Capability::ExternalNavigation,
				Capability::ContextMenus,
				Capability::InterfaceTweaks,
				Capability::MediaLinks,
				Capability::AttachmentContents,
				Capability::ServerProfiles,
			],
			app_events: &[
				AppEventKind::Ready,
				AppEventKind::Navigation,
				AppEventKind::Context,
				AppEventKind::Connection,
				AppEventKind::Voice,
				AppEventKind::Settings,
				AppEventKind::Account,
				AppEventKind::Channels,
				AppEventKind::Members,
				AppEventKind::Presence,
				AppEventKind::ReadState,
				AppEventKind::MessageDetails,
				AppEventKind::Relationships,
				AppEventKind::Threads,
				AppEventKind::Roles,
				AppEventKind::Permissions,
				AppEventKind::Recovered,
				AppEventKind::Reactions,
				AppEventKind::Pins,
				AppEventKind::Typing,
				AppEventKind::Polls,
			],
			interface_tweaks: InterfaceTweak::ALL,
			api_versions: crate::SUPPORTED_API_VERSIONS,
			features: crate::FEATURES.iter().copied().collect(),
			surfaces: crate::SURFACES,
			slots: &[],
			request_kinds: crate::REQUEST_KINDS,
			limits: ResourceLimits::DEFAULT,
		}
	}
}
