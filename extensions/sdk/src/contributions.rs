//! API v2 interface contributions: small items Harmony draws in named slots for you.
//!
//! Return them in [`crate::ExtensionOutput::contributions`] from any action, event,
//! activation or tick. Harmony keeps them until you remove them, they expire, the extension is
//! disabled or the account changes; it draws and animates them without running your code per
//! frame. Declare the `ui_contributions` capability and every slot you use in `slots`.
//!
//! ```
//! use harmony_extension_sdk::{ContributionItem, ContributionOp, ContributionTarget, ExtensionOutput, Slot};
//! let mut output = ExtensionOutput::default();
//! output.contribute(ContributionOp::set(
//!     Slot::UserBadge,
//!     "friend-42",
//!     ContributionTarget::user("42"),
//!     ContributionItem::text("Friend").color("#3BA55C"),
//! ));
//! ```
use serde::{Deserialize, Serialize};

/// Where an item appears. Each slot accepts one kind of target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Slot {
	/// A short badge after a message's timestamp; target a message. Up to 32 characters.
	#[serde(rename = "message.badge")]
	MessageBadge,
	/// One line under a message's content; target a message. Up to 300 characters.
	#[serde(rename = "message.accessory")]
	MessageAccessory,
	/// A short badge after a person's name; target a user. Up to 32 characters.
	#[serde(rename = "user.badge")]
	UserBadge,
	/// An icon button beside the composer that runs one of your `panel` actions; target
	/// [`ContributionTarget::App`]. The text is the button's accessible name.
	#[serde(rename = "composer.button")]
	ComposerButton,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContributionTarget {
	Message { message_id: String },
	User { user_id: String },
	App,
}

impl ContributionTarget {
	pub fn message(id: impl Into<String>) -> Self {
		Self::Message {
			message_id: id.into(),
		}
	}
	pub fn user(id: impl Into<String>) -> Self {
		Self::User { user_id: id.into() }
	}
}

/// Icons from Harmony's own set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionIcon {
	Tag,
	Star,
	Info,
	Warning,
	Check,
	Sparkle,
	Link,
	Pin,
	Bell,
	Shield,
	Search,
	External,
	Copy,
	Code,
	Server,
	Eye,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationKind {
	Pulse,
	Shimmer,
	Rainbow,
}

/// An animation Harmony runs itself; `period_ms` is 500 to 10,000.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Animation {
	pub kind: AnimationKind,
	pub period_ms: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContributionItem {
	pub text: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tooltip: Option<String>,
	/// `#RRGGBB`.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub icon: Option<ContributionIcon>,
	/// `composer.button` only: one of your declared `panel` action IDs.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub action: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub animation: Option<Animation>,
}

impl ContributionItem {
	pub fn text(text: impl Into<String>) -> Self {
		Self {
			text: text.into(),
			..Default::default()
		}
	}
	pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
		self.tooltip = Some(tooltip.into());
		self
	}
	pub fn color(mut self, color: impl Into<String>) -> Self {
		self.color = Some(color.into());
		self
	}
	pub fn icon(mut self, icon: ContributionIcon) -> Self {
		self.icon = Some(icon);
		self
	}
	pub fn action(mut self, action: impl Into<String>) -> Self {
		self.action = Some(action.into());
		self
	}
	pub fn animate(mut self, kind: AnimationKind, period_ms: u32) -> Self {
		self.animation = Some(Animation { kind, period_ms });
		self
	}
}

/// One change to your contributions. Keys are yours alone: other extensions can use the same
/// key without touching your items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ContributionOp {
	Set {
		slot: Slot,
		key: String,
		target: ContributionTarget,
		item: ContributionItem,
		/// Expires after this long, at most seven days.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		ttl_ms: Option<u64>,
	},
	Remove {
		slot: Slot,
		key: String,
	},
	/// Removes your items in one slot, or in every slot.
	Clear {
		#[serde(default, skip_serializing_if = "Option::is_none")]
		slot: Option<Slot>,
	},
}

impl ContributionOp {
	pub fn set(
		slot: Slot,
		key: impl Into<String>,
		target: ContributionTarget,
		item: ContributionItem,
	) -> Self {
		Self::Set {
			slot,
			key: key.into(),
			target,
			item,
			ttl_ms: None,
		}
	}
	pub fn remove(slot: Slot, key: impl Into<String>) -> Self {
		Self::Remove {
			slot,
			key: key.into(),
		}
	}
}
