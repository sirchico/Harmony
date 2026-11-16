//! API v2 UI contributions: small, declarative pieces of interface an extension adds to named
//! slots. Harmony draws and animates them; extension code never runs per frame. Each item is
//! owned by one extension, keyed within its slot, bounded, optionally expiring, and removed
//! when the extension is disabled or the account changes.
use crate::{Capability, Error, Manifest};
use serde::{Deserialize, Serialize};

/// Where a contribution appears. New slots are additive; unknown slots fail validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Slot {
	/// A short badge after a message's timestamp. Target: a message.
	#[serde(rename = "message.badge")]
	MessageBadge,
	/// One line of text under a message's content. Target: a message.
	#[serde(rename = "message.accessory")]
	MessageAccessory,
	/// A short badge after a person's name in messages and the member list. Target: a user.
	#[serde(rename = "user.badge")]
	UserBadge,
	/// An icon button beside the composer's buttons that runs a declared `panel` action.
	#[serde(rename = "composer.button")]
	ComposerButton,
}

impl Slot {
	pub const ALL: [Slot; 4] = [
		Slot::MessageBadge,
		Slot::MessageAccessory,
		Slot::UserBadge,
		Slot::ComposerButton,
	];
	pub fn name(self) -> &'static str {
		match self {
			Slot::MessageBadge => "message.badge",
			Slot::MessageAccessory => "message.accessory",
			Slot::UserBadge => "user.badge",
			Slot::ComposerButton => "composer.button",
		}
	}
	/// Longest `text` for this slot, in characters.
	pub fn max_text(self) -> usize {
		match self {
			Slot::MessageAccessory => 2000,
			Slot::ComposerButton => 24,
			Slot::MessageBadge | Slot::UserBadge => 32,
		}
	}
	/// How many items of one target are drawn; the rest are summarized as "+N".
	pub fn shown_per_target(self) -> usize {
		match self {
			Slot::MessageAccessory => 2,
			Slot::ComposerButton => 4,
			Slot::MessageBadge | Slot::UserBadge => 3,
		}
	}
}

/// What a contribution is attached to.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContributionTarget {
	Message {
		message_id: String,
	},
	User {
		user_id: String,
	},
	/// Slots that are not attached to anything, such as composer buttons.
	App,
}

/// Icons from Harmony's own set; extensions cannot supply images to these slots.
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
	Globe,
	Translate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationKind {
	/// Fades the item in and out.
	Pulse,
	/// A light band moves across the item.
	Shimmer,
	/// The color cycles through the hue wheel.
	Rainbow,
}

/// An animation Harmony runs on its own; the extension is not invoked per frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
	pub kind: AnimationKind,
	/// One cycle, 500 to 10,000 milliseconds.
	pub period_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContributionItem {
	pub text: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tooltip: Option<String>,
	/// `#RRGGBB`; Harmony keeps text readable against the theme.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub icon: Option<ContributionIcon>,
	/// `composer.button` only: the declared `panel` action the button runs.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub action: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub animation: Option<Animation>,
	/// `message.accessory` only: a smaller second line, such as "translated from Spanish".
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub note: Option<String>,
	/// `message.accessory` only: adds a Dismiss link that hides the item.
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub dismissible: bool,
	/// `message.accessory` only: draws the text in italics, like a quotation.
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub italic: bool,
	/// `composer.button` only: a declared `panel` action run by a right-click or Shift-click.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub secondary_action: Option<String>,
}

/// One change to the extension's contributions, applied after its output is validated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
// A wire type built once per output; boxing the item would only complicate authors' code.
#[allow(clippy::large_enum_variant)]
pub enum ContributionOp {
	/// Adds or replaces the item with this key in the slot.
	Set {
		slot: Slot,
		key: String,
		target: ContributionTarget,
		item: ContributionItem,
		/// Removes the item after this long; at most seven days. Absent keeps it until the
		/// extension removes it, is disabled, or the account changes.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		ttl_ms: Option<u64>,
	},
	Remove {
		slot: Slot,
		key: String,
	},
	/// Removes every item of one slot, or of every slot.
	Clear {
		#[serde(default, skip_serializing_if = "Option::is_none")]
		slot: Option<Slot>,
	},
}

pub const MAX_CONTRIBUTION_OPS: usize = 64;
/// Items one extension may keep in one slot.
pub const MAX_CONTRIBUTIONS_PER_SLOT: usize = 256;
/// Items one extension may keep in all slots together.
pub const MAX_CONTRIBUTIONS_PER_EXTENSION: usize = 512;
pub const MAX_CONTRIBUTION_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

pub(crate) fn valid_key(key: &str) -> bool {
	!key.is_empty()
		&& key.len() <= 64
		&& key
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

fn short_text(text: &str, max_chars: usize) -> bool {
	!text.trim().is_empty()
		&& text.chars().count() <= max_chars
		&& !text.chars().any(char::is_control)
}

/// Accessory lines may wrap onto a few lines of their own.
fn accessory_text(text: &str) -> bool {
	!text.trim().is_empty()
		&& text.chars().count() <= Slot::MessageAccessory.max_text()
		&& text.lines().count() <= 8
		&& !text.chars().any(|c| c.is_control() && c != '\n')
}

impl ContributionOp {
	pub fn slot(&self) -> Option<Slot> {
		match self {
			Self::Set { slot, .. } | Self::Remove { slot, .. } => Some(*slot),
			Self::Clear { slot } => *slot,
		}
	}

	pub(crate) fn validate(&self, manifest: &Manifest) -> Result<(), Error> {
		if let Some(slot) = self.slot()
			&& !manifest.slots.contains(&slot)
		{
			return Err(Error::Capability);
		}
		match self {
			Self::Set {
				slot,
				key,
				target,
				item,
				ttl_ms,
			} => {
				let text_ok = if *slot == Slot::MessageAccessory {
					accessory_text(&item.text)
				} else {
					short_text(&item.text, slot.max_text())
				};
				let accessory_only = item.note.is_some() || item.dismissible || item.italic;
				if !valid_key(key)
					|| !text_ok || (accessory_only && *slot != Slot::MessageAccessory)
					|| item
						.note
						.as_deref()
						.is_some_and(|note| !short_text(note, 200))
					|| item
						.tooltip
						.as_deref()
						.is_some_and(|tooltip| !short_text(tooltip, 256))
					|| item
						.color
						.as_deref()
						.is_some_and(|color| color.len() != 7 || crate::parse_color(color).is_err())
					|| item
						.animation
						.is_some_and(|animation| !(500..=10_000).contains(&animation.period_ms))
					|| ttl_ms.is_some_and(|ttl| ttl == 0 || ttl > MAX_CONTRIBUTION_TTL_MS)
				{
					return Err(Error::Invalid);
				}
				let target_ok = match (slot, target) {
					(
						Slot::MessageBadge | Slot::MessageAccessory,
						ContributionTarget::Message { message_id },
					) => crate::app::entity_id(message_id).is_ok(),
					(Slot::UserBadge, ContributionTarget::User { user_id }) => {
						crate::app::entity_id(user_id).is_ok()
					}
					(Slot::ComposerButton, ContributionTarget::App) => true,
					_ => false,
				};
				if !target_ok {
					return Err(Error::Invalid);
				}
				match (&item.action, slot) {
					(Some(action), Slot::ComposerButton) => {
						if !manifest.actions.iter().any(|declared| {
							declared.id == *action && declared.surface == crate::Surface::Panel
						}) {
							return Err(Error::Invalid);
						}
					}
					(None, Slot::ComposerButton) | (Some(_), _) => return Err(Error::Invalid),
					(None, _) => {}
				}
				if let Some(action) = &item.secondary_action
					&& (*slot != Slot::ComposerButton
						|| !manifest.actions.iter().any(|declared| {
							declared.id == *action && declared.surface == crate::Surface::Panel
						})) {
					return Err(Error::Invalid);
				}
			}
			Self::Remove { key, .. } => {
				if !valid_key(key) {
					return Err(Error::Invalid);
				}
			}
			Self::Clear { .. } => {}
		}
		Ok(())
	}
}

/// Validates the `slots` an API v2 manifest declares.
pub(crate) fn validate_manifest(manifest: &Manifest) -> Result<(), Error> {
	if manifest.slots.is_empty() {
		return Ok(());
	}
	if !manifest.capabilities.contains(&Capability::UiContributions) {
		return Err(Error::Capability);
	}
	let mut seen = std::collections::BTreeSet::new();
	if !manifest.slots.iter().all(|slot| seen.insert(*slot)) {
		return Err(Error::Invalid);
	}
	Ok(())
}

/// Validates the contribution changes in one output.
pub(crate) fn validate_output(ops: &[ContributionOp], manifest: &Manifest) -> Result<(), Error> {
	if ops.is_empty() {
		return Ok(());
	}
	if manifest.api_version < crate::API_V2
		|| !manifest.capabilities.contains(&Capability::UiContributions)
	{
		return Err(Error::Capability);
	}
	if ops.len() > MAX_CONTRIBUTION_OPS {
		return Err(Error::Limit);
	}
	ops.iter().try_for_each(|op| op.validate(manifest))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn manifest(api: u32, capabilities: &str, slots: &str) -> Manifest {
		serde_json::from_str(&format!(
			r#"{{"api_version":{api},"id":"probe","name":"Probe","version":"1.0.0","author":"Synthetic",
			"license":"MIT","source":"https://example.org/probe","kind":"plugin",
			"capabilities":[{capabilities}],"slots":[{slots}],
			"actions":[{{"id":"open","label":"Open","surface":"panel"}}]}}"#
		))
		.unwrap()
	}
	fn op(json: &str) -> ContributionOp {
		serde_json::from_str(json).unwrap()
	}

	#[test]
	fn contributions_need_the_grant_a_declared_slot_and_the_slots_target() {
		let granted = manifest(
			2,
			r#""ui_contributions""#,
			r#""user.badge","composer.button""#,
		);
		assert!(granted.validate().is_ok());
		// Slots need the capability, and v1 manifests cannot declare them.
		assert!(manifest(2, "", r#""user.badge""#).validate().is_err());
		assert!(manifest(1, "", r#""user.badge""#).validate().is_err());
		let badge = |target: &str| {
			op(&format!(
				r##"{{"op":"set","slot":"user.badge","key":"k","target":{target},"item":{{"text":"Hi","color":"#3BA55C"}}}}"##
			))
		};
		assert!(validate_output(&[badge(r#"{"kind":"user","user_id":"5"}"#)], &granted).is_ok());
		assert!(
			validate_output(&[badge(r#"{"kind":"message","message_id":"5"}"#)], &granted).is_err()
		);
		assert!(validate_output(&[badge(r#"{"kind":"user","user_id":"me"}"#)], &granted).is_err());
		let undeclared = op(
			r#"{"op":"set","slot":"message.badge","key":"k","target":{"kind":"message","message_id":"5"},"item":{"text":"Hi"}}"#,
		);
		assert!(matches!(
			validate_output(&[undeclared], &granted),
			Err(Error::Capability)
		));
		let button = |action: &str| {
			op(&format!(
				r#"{{"op":"set","slot":"composer.button","key":"b","target":{{"kind":"app"}},"item":{{"text":"Open","action":"{action}"}}}}"#
			))
		};
		assert!(validate_output(&[button("open")], &granted).is_ok());
		assert!(validate_output(&[button("missing")], &granted).is_err());
		let ttl = op(
			r#"{"op":"set","slot":"user.badge","key":"k","target":{"kind":"user","user_id":"5"},"item":{"text":"Hi"},"ttl_ms":0}"#,
		);
		assert!(validate_output(&[ttl], &granted).is_err());
		let v1 = manifest(1, "", "");
		assert!(validate_output(&[op(r#"{"op":"clear"}"#)], &v1).is_err());
	}
}
