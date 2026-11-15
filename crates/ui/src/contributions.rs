//! Extension UI contributions (API v2): the bounded store the bridge fills from extension
//! output, the immutable view widgets read each frame, and the painters for each slot.
//! Extension code never runs here; animations are computed from the frame time.
use crate::design;
use extensions::{
	AnimationKind, ContributionIcon, ContributionItem, ContributionOp, ContributionTarget,
	MAX_CONTRIBUTIONS_PER_EXTENSION, MAX_CONTRIBUTIONS_PER_SLOT, Slot,
};
use model::Id;
use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

const KEY: &str = "extension-contributions";

#[derive(Clone, Debug)]
struct Entry {
	target: ContributionTarget,
	item: ContributionItem,
	expires: Option<Instant>,
}

/// Every live contribution, keyed by (slot, extension, key). Owned by the extension bridge.
#[derive(Default)]
pub struct ContributionStore {
	items: BTreeMap<(Slot, String, String), Entry>,
	view: Arc<ContributionView>,
	revision: u64,
}

/// One drawable item, with its owner for actions and tooltips.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
	pub extension: String,
	pub item: ContributionItem,
}

/// Items for one target in one slot: the first few in a stable order, and how many more.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shelf {
	pub items: Vec<Shown>,
	pub more: usize,
}

/// What widgets read this frame. Rebuilt only when the store changes.
#[derive(Debug, Default)]
pub struct ContributionView {
	pub revision: u64,
	messages: HashMap<(Slot, u64), Shelf>,
	users: HashMap<u64, Shelf>,
	composer: Shelf,
	/// Per-message hash of everything that can change a row's height.
	accessory_prints: HashMap<u64, u64>,
	animated: bool,
}

impl ContributionStore {
	/// Applies one output's changes for `extension`. The whole batch is refused if it would
	/// exceed the per-slot or per-extension limits, leaving earlier items unchanged.
	pub fn apply(
		&mut self,
		extension: &str,
		ops: &[ContributionOp],
		now: Instant,
	) -> Result<(), String> {
		if ops.is_empty() {
			return Ok(());
		}
		let mut next = self.items.clone();
		for op in ops {
			match op {
				ContributionOp::Set {
					slot,
					key,
					target,
					item,
					ttl_ms,
				} => {
					next.insert(
						(*slot, extension.to_owned(), key.clone()),
						Entry {
							target: target.clone(),
							item: item.clone(),
							expires: ttl_ms.map(|ttl| now + Duration::from_millis(ttl)),
						},
					);
				}
				ContributionOp::Remove { slot, key } => {
					next.remove(&(*slot, extension.to_owned(), key.clone()));
				}
				ContributionOp::Clear { slot } => {
					next.retain(|(item_slot, owner, _), _| {
						owner != extension || slot.is_some_and(|slot| slot != *item_slot)
					});
				}
			}
		}
		let owned = next.keys().filter(|(_, owner, _)| owner == extension);
		let mut per_slot = BTreeMap::<Slot, usize>::new();
		let mut total = 0;
		for (slot, _, _) in owned {
			total += 1;
			*per_slot.entry(*slot).or_default() += 1;
		}
		if total > MAX_CONTRIBUTIONS_PER_EXTENSION
			|| per_slot
				.values()
				.any(|count| *count > MAX_CONTRIBUTIONS_PER_SLOT)
		{
			return Err(format!(
				"{extension} tried to add more interface items than its limit; nothing was changed"
			));
		}
		self.items = next;
		self.rebuild();
		Ok(())
	}

	/// Removes everything an extension contributed, as when it is disabled or fails.
	pub fn remove_extension(&mut self, extension: &str) {
		let before = self.items.len();
		self.items.retain(|(_, owner, _), _| owner != extension);
		if self.items.len() != before {
			self.rebuild();
		}
	}

	/// Removes every contribution, as when the account or connection changes.
	pub fn clear(&mut self) {
		if !self.items.is_empty() {
			self.items.clear();
			self.rebuild();
		}
	}

	/// Drops expired items and returns when the next one expires.
	pub fn expire(&mut self, now: Instant) -> Option<Instant> {
		let before = self.items.len();
		self.items
			.retain(|_, entry| entry.expires.is_none_or(|expires| expires > now));
		if self.items.len() != before {
			self.rebuild();
		}
		self.items.values().filter_map(|entry| entry.expires).min()
	}

	pub fn view(&self) -> Arc<ContributionView> {
		self.view.clone()
	}

	pub fn len(&self) -> usize {
		self.items.len()
	}

	pub fn is_empty(&self) -> bool {
		self.items.is_empty()
	}

	fn rebuild(&mut self) {
		self.revision += 1;
		let mut view = ContributionView {
			revision: self.revision,
			..Default::default()
		};
		// BTreeMap order is (slot, extension, key): stable regardless of arrival order, so two
		// extensions targeting the same thing always appear in the same order.
		let push = |shelf: &mut Shelf, slot: Slot, extension: &str, item: &ContributionItem| {
			if shelf.items.len() < slot.shown_per_target() {
				shelf.items.push(Shown {
					extension: extension.to_owned(),
					item: item.clone(),
				});
			} else {
				shelf.more += 1;
			}
		};
		for ((slot, extension, _), entry) in &self.items {
			view.animated |= entry.item.animation.is_some();
			match &entry.target {
				ContributionTarget::Message { message_id } => {
					let Ok(id) = message_id.parse::<u64>() else {
						continue;
					};
					push(
						view.messages.entry((*slot, id)).or_default(),
						*slot,
						extension,
						&entry.item,
					);
					// Both message slots can change a row's height (badges sit under rows
					// that continue the previous message).
					{
						let print = view.accessory_prints.entry(id).or_default();
						let mut hasher = DefaultHasher::new();
						print.hash(&mut hasher);
						slot.hash(&mut hasher);
						extension.hash(&mut hasher);
						entry.item.text.hash(&mut hasher);
						*print = hasher.finish();
					}
				}
				ContributionTarget::User { user_id } => {
					if let Ok(id) = user_id.parse::<u64>() {
						push(
							view.users.entry(id).or_default(),
							*slot,
							extension,
							&entry.item,
						);
					}
				}
				ContributionTarget::App => push(&mut view.composer, *slot, extension, &entry.item),
			}
		}
		self.view = Arc::new(view);
	}
}

impl ContributionView {
	pub fn message(&self, slot: Slot, id: Id) -> Option<&Shelf> {
		self.messages.get(&(slot, id.0))
	}
	pub fn user(&self, id: Id) -> Option<&Shelf> {
		self.users.get(&id.0)
	}
	pub fn composer(&self) -> &Shelf {
		&self.composer
	}
	/// Changes whenever a message's badges or lines change, so cached row heights can be
	/// dropped for exactly that message.
	pub fn accessory_print(&self, id: Id) -> u64 {
		self.accessory_prints.get(&id.0).copied().unwrap_or(0)
	}
	pub fn accessory_ids(&self) -> impl Iterator<Item = u64> + '_ {
		self.accessory_prints.keys().copied()
	}
}

/// Publishes this frame's view for [`current`] to read.
pub(crate) fn publish(ctx: &egui::Context, view: &Arc<ContributionView>) {
	ctx.data_mut(|data| data.insert_temp(egui::Id::unique(KEY), view.clone()));
}

pub(crate) fn current(ctx: &egui::Context) -> Arc<ContributionView> {
	ctx.data(|data| data.get_temp::<Arc<ContributionView>>(egui::Id::unique(KEY)))
		.unwrap_or_default()
}

pub(crate) fn icon(icon: ContributionIcon) -> crate::icons::Icon {
	use crate::icons::Icon;
	match icon {
		ContributionIcon::Tag => Icon::Tag,
		ContributionIcon::Star => Icon::StarFill,
		ContributionIcon::Info => Icon::Help,
		ContributionIcon::Warning => Icon::ShieldWarning,
		ContributionIcon::Check => Icon::Check,
		ContributionIcon::Sparkle => Icon::Sparkle,
		ContributionIcon::Link => Icon::Link,
		ContributionIcon::Pin => Icon::Pin,
		ContributionIcon::Bell => Icon::Bell,
		ContributionIcon::Shield => Icon::Verified,
	}
}

fn base_color(item: &ContributionItem, fallback: egui::Color32) -> egui::Color32 {
	item.color
		.as_deref()
		.and_then(|color| extensions::parse_color(color).ok())
		.map_or(fallback, |[r, g, b, _]| egui::Color32::from_rgb(r, g, b))
}

/// The item's color at time `t` seconds, with its animation applied, and its opacity.
fn animated(item: &ContributionItem, fallback: egui::Color32, t: f64) -> (egui::Color32, f32) {
	let color = base_color(item, fallback);
	let Some(animation) = item.animation else {
		return (color, 1.0);
	};
	let phase = (t * 1000.0 / f64::from(animation.period_ms)).fract() as f32;
	match animation.kind {
		AnimationKind::Pulse => (
			color,
			0.55 + 0.45 * (phase * std::f32::consts::TAU).cos().abs(),
		),
		AnimationKind::Shimmer => (color, 1.0),
		AnimationKind::Rainbow => {
			let hsva = egui::ecolor::Hsva::new(phase, 0.65, 0.95, 1.0);
			(hsva.into(), 1.0)
		}
	}
}

/// Keeps animating items moving without repainting idle windows.
fn keep_animating(ui: &egui::Ui, view: &ContributionView) {
	if view.animated {
		ui.ctx().request_repaint_after(Duration::from_millis(33));
	}
}

/// Small pill badges for a message header or a name row. Returns their total width.
pub(crate) fn badges(ui: &mut egui::Ui, view: &ContributionView, shelf: &Shelf) {
	let colors = design::palette(ui);
	let t = ui.input(|input| input.time);
	for shown in &shelf.items {
		let item = &shown.item;
		let (color, alpha) = animated(item, colors.accent, t);
		let font = egui::FontId::proportional(11.0);
		let galley = ui
			.painter()
			.layout_no_wrap(item.text.clone(), font, colors.text_strong);
		let icon_width = if item.icon.is_some() { 12.0 } else { 0.0 };
		let size = egui::vec2(galley.size().x + icon_width + 10.0, 16.0);
		let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
		let fill = design::mix(colors.raised, color, 0.28).gamma_multiply(alpha);
		ui.painter().rect(
			rect,
			4,
			fill,
			egui::Stroke::new(1.0, color.gamma_multiply(0.7 * alpha)),
			egui::StrokeKind::Inside,
		);
		if item
			.animation
			.is_some_and(|a| a.kind == AnimationKind::Shimmer)
		{
			let period = f64::from(item.animation.map_or(1000, |a| a.period_ms)) / 1000.0;
			let x = rect.left() + rect.width() * ((t / period).fract() as f32);
			let band = egui::Rect::from_center_size(
				egui::pos2(x, rect.center().y),
				egui::vec2(8.0, rect.height()),
			)
			.intersect(rect);
			ui.painter()
				.rect_filled(band, 2, egui::Color32::WHITE.gamma_multiply(0.18));
		}
		let mut left = rect.left() + 5.0;
		if let Some(glyph) = item.icon {
			crate::icons::paint(
				ui.painter(),
				icon(glyph),
				egui::Rect::from_min_size(
					egui::pos2(left, rect.center().y - 5.0),
					egui::Vec2::splat(10.0),
				),
				colors.text_strong.gamma_multiply(alpha),
			);
			left += icon_width;
		}
		ui.painter().galley(
			egui::pos2(left, rect.center().y - galley.size().y / 2.0),
			galley,
			colors.text_strong.gamma_multiply(alpha.max(0.8)),
		);
		let tooltip = item.tooltip.clone();
		let owner = shown.extension.clone();
		response.on_hover_ui(|ui| {
			if let Some(tooltip) = tooltip {
				ui.label(tooltip);
			}
			ui.label(
				egui::RichText::new(format!("From the {owner} extension"))
					.size(11.0)
					.color(colors.muted),
			);
		});
	}
	if shelf.more > 0 {
		ui.label(
			egui::RichText::new(format!("+{}", shelf.more))
				.size(11.0)
				.color(colors.muted),
		);
	}
	keep_animating(ui, view);
}

/// Width [`badges`] will take, for reserving room before a truncated name.
pub(crate) fn badges_width(ui: &egui::Ui, shelf: &Shelf) -> f32 {
	let spacing = ui.spacing().item_spacing.x;
	shelf
		.items
		.iter()
		.map(|shown| {
			let galley = ui.painter().layout_no_wrap(
				shown.item.text.clone(),
				egui::FontId::proportional(11.0),
				egui::Color32::WHITE,
			);
			galley.size().x + if shown.item.icon.is_some() { 12.0 } else { 0.0 } + 10.0 + spacing
		})
		.sum::<f32>()
		+ if shelf.more > 0 { 24.0 } else { 0.0 }
}

/// Lines of text under a message's content.
pub(crate) fn accessories(ui: &mut egui::Ui, view: &ContributionView, shelf: &Shelf) {
	let colors = design::palette(ui);
	let t = ui.input(|input| input.time);
	for shown in &shelf.items {
		let item = &shown.item;
		let (color, alpha) = animated(item, colors.muted, t);
		ui.horizontal(|ui| {
			ui.spacing_mut().item_spacing.x = 6.0;
			let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
			crate::icons::paint(
				ui.painter(),
				icon(item.icon.unwrap_or(ContributionIcon::Sparkle)),
				rect,
				color.gamma_multiply(alpha),
			);
			let label = ui.add(
				egui::Label::new(
					egui::RichText::new(&item.text)
						.size(13.0)
						.color(design::mix(colors.text, color, 0.35).gamma_multiply(alpha)),
				)
				.wrap(),
			);
			let owner = shown.extension.clone();
			let tooltip = item.tooltip.clone();
			label.on_hover_ui(|ui| {
				if let Some(tooltip) = tooltip {
					ui.label(tooltip);
				}
				ui.label(
					egui::RichText::new(format!("From the {owner} extension"))
						.size(11.0)
						.color(colors.muted),
				);
			});
		});
	}
	if shelf.more > 0 {
		ui.label(
			egui::RichText::new(format!("{} more from extensions", shelf.more))
				.size(11.0)
				.color(colors.muted),
		);
	}
	keep_animating(ui, view);
}

#[cfg(test)]
mod tests {
	use super::*;
	use extensions::{Animation, ContributionItem};

	fn badge(text: &str) -> ContributionItem {
		ContributionItem {
			text: text.into(),
			tooltip: None,
			color: None,
			icon: None,
			action: None,
			animation: None,
		}
	}
	fn set(slot: Slot, key: &str, user: &str, text: &str, ttl_ms: Option<u64>) -> ContributionOp {
		ContributionOp::Set {
			slot,
			key: key.into(),
			target: ContributionTarget::User {
				user_id: user.into(),
			},
			item: badge(text),
			ttl_ms,
		}
	}

	#[test]
	fn items_are_owned_ordered_capped_expiring_and_removed_with_their_extension() {
		let mut store = ContributionStore::default();
		let now = Instant::now();
		// Arrival order must not decide display order: extension, then key.
		store
			.apply("zeta", &[set(Slot::UserBadge, "a", "7", "Z", None)], now)
			.unwrap();
		store
			.apply(
				"alpha",
				&[
					set(Slot::UserBadge, "b", "7", "A2", None),
					set(Slot::UserBadge, "a", "7", "A1", Some(1000)),
					set(Slot::UserBadge, "c", "7", "A3", None),
				],
				now,
			)
			.unwrap();
		let shelf = store.view().user(Id(7)).cloned().unwrap();
		let texts: Vec<_> = shelf.items.iter().map(|s| s.item.text.as_str()).collect();
		assert_eq!((texts, shelf.more), (vec!["A1", "A2", "A3"], 1));
		// Same key replaces, other owners' items with the same key are untouched.
		store
			.apply("zeta", &[set(Slot::UserBadge, "a", "7", "Z2", None)], now)
			.unwrap();
		assert_eq!(store.len(), 4);
		assert_eq!(
			store.expire(now + Duration::from_millis(999)),
			Some(now + Duration::from_secs(1))
		);
		assert_eq!(store.expire(now + Duration::from_secs(1)), None);
		assert_eq!(store.len(), 3);
		store.remove_extension("alpha");
		let shelf = store.view().user(Id(7)).cloned().unwrap();
		assert_eq!(shelf.items[0].item.text, "Z2");
		assert_eq!(shelf.more, 0);
	}

	#[test]
	fn a_batch_over_the_limit_changes_nothing() {
		let mut store = ContributionStore::default();
		let now = Instant::now();
		store
			.apply(
				"alpha",
				&[set(Slot::UserBadge, "keep", "1", "Kept", None)],
				now,
			)
			.unwrap();
		let revision = store.view().revision;
		let too_many: Vec<_> = (0..MAX_CONTRIBUTIONS_PER_SLOT)
			.map(|n| set(Slot::UserBadge, &format!("k{n}"), "1", "x", None))
			.collect();
		assert!(store.apply("alpha", &too_many, now).is_err());
		assert_eq!((store.len(), store.view().revision), (1, revision));
		// Clearing one slot leaves the extension's other slots alone.
		store
			.apply(
				"alpha",
				&[
					ContributionOp::Set {
						slot: Slot::MessageAccessory,
						key: "note".into(),
						target: ContributionTarget::Message {
							message_id: "5".into(),
						},
						item: ContributionItem {
							animation: Some(Animation {
								kind: AnimationKind::Pulse,
								period_ms: 1000,
							}),
							..badge("Line")
						},
						ttl_ms: None,
					},
					ContributionOp::Clear {
						slot: Some(Slot::UserBadge),
					},
				],
				now,
			)
			.unwrap();
		let view = store.view();
		assert!(view.user(Id(1)).is_none());
		assert_ne!(view.accessory_print(Id(5)), 0);
		assert!(view.animated);
	}

	/// Items the bridge sets are drawn by the real chat frame: a user badge beside the
	/// author's name and a line under their latest message; removing them takes them away.
	#[test]
	fn client_frame_draws_and_removes_contributed_items() {
		fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
			match shape {
				egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
				egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
				_ => {}
			}
		}
		let ctx = egui::Context::default();
		crate::fonts::install(&ctx);
		let mut view = crate::MessagingUi::default();
		let mut state = test_support::demo_state();
		let message = state.timeline.display_iter().last().unwrap().clone();
		let mut frame = |view: &mut crate::MessagingUi| {
			let mut drawn = Vec::new();
			for _ in 0..3 {
				let output = ctx.run_ui(
					egui::RawInput {
						screen_rect: Some(egui::Rect::from_min_size(
							egui::Pos2::ZERO,
							egui::vec2(1400.0, 900.0),
						)),
						..Default::default()
					},
					|ui| {
						view.show(ui, &mut state);
					},
				);
				drawn.clear();
				for clipped in &output.shapes {
					texts(&clipped.shape, &mut drawn);
				}
				output.drop_without_applying_deltas();
			}
			drawn
		};
		let mut store = ContributionStore::default();
		store
			.apply(
				"probe",
				&[
					ContributionOp::Set {
						slot: Slot::UserBadge,
						key: "author".into(),
						target: ContributionTarget::User {
							user_id: message.author.id.to_string(),
						},
						item: badge("ProbeTag"),
						ttl_ms: None,
					},
					ContributionOp::Set {
						slot: Slot::MessageAccessory,
						key: "note".into(),
						target: ContributionTarget::Message {
							message_id: message.id.to_string(),
						},
						item: badge("Probe line under the message"),
						ttl_ms: None,
					},
				],
				Instant::now(),
			)
			.unwrap();
		view.set_contributions(store.view());
		let drawn = frame(&mut view);
		assert!(drawn.iter().any(|text| text == "ProbeTag"), "{drawn:?}");
		assert!(
			drawn
				.iter()
				.any(|text| text == "Probe line under the message")
		);
		store.remove_extension("probe");
		view.set_contributions(store.view());
		let drawn = frame(&mut view);
		assert!(!drawn.iter().any(|text| text.starts_with("Probe")));
	}
}
