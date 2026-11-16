//! API v2 item actions: extension buttons in the message hover toolbar, entries (with
//! submenus) in image menus, and buttons on matching file attachments. Widgets read the
//! enabled actions from frame data and record one click, which the extension UI dispatches
//! on the next frame like any other menu choice.
use crate::extensions_ui::MessageTarget;
use extensions::{ActionChoice, AttachmentFilter, ContributionIcon, MediaLink, Surface};
use std::sync::Arc;

const ACTIONS: &str = "extension-item-actions";
const CHOICE: &str = "extension-item-choice";
const COPIED: &str = "extension-item-copied";
/// How long a button shows its check mark after a direct copy, like other copy buttons.
const COPIED_SECONDS: f64 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemAction {
	pub plugin: String,
	pub action: String,
	pub label: String,
	pub surface: Surface,
	pub icon: Option<ContributionIcon>,
	pub choices: Vec<ActionChoice>,
	pub accepts: Option<AttachmentFilter>,
}

/// One click on an item action, with what it was chosen on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemChoice {
	pub plugin: String,
	pub action: String,
	pub button: Option<String>,
	pub media: Option<MediaLink>,
	pub message: Option<(String, MessageTarget)>,
}

pub(crate) fn publish(ctx: &egui::Context, actions: &Arc<Vec<ItemAction>>) {
	ctx.data_mut(|data| data.insert_temp(egui::Id::unique(ACTIONS), actions.clone()));
}

/// Enabled actions for one placement, in a stable order.
pub(crate) fn actions(ctx: &egui::Context, surface: Surface) -> Vec<ItemAction> {
	ctx.data(|data| data.get_temp::<Arc<Vec<ItemAction>>>(egui::Id::unique(ACTIONS)))
		.map(|actions| {
			actions
				.iter()
				.filter(|action| action.surface == surface)
				.cloned()
				.collect()
		})
		.unwrap_or_default()
}

pub(crate) fn choose(ctx: &egui::Context, choice: ItemChoice) {
	ctx.data_mut(|data| data.insert_temp(egui::Id::unique(CHOICE), Some(choice)));
}

pub(crate) fn take_choice(ctx: &egui::Context) -> Option<ItemChoice> {
	ctx.data_mut(|data| {
		let choice = data
			.get_temp::<Option<ItemChoice>>(egui::Id::unique(CHOICE))
			.flatten();
		data.remove::<Option<ItemChoice>>(egui::Id::unique(CHOICE));
		choice
	})
}

/// Records a direct copy for `key` (an attachment or message ID) so its button shows a check.
pub(crate) fn mark_copied(ctx: &egui::Context, key: &str) {
	let now = ctx.input(|input| input.time);
	ctx.data_mut(|data| {
		data.insert_temp(egui::Id::unique(COPIED), Some((key.to_owned(), now)));
	});
}

pub(crate) fn copied_recently(ctx: &egui::Context, key: &str) -> bool {
	let now = ctx.input(|input| input.time);
	let copied = ctx
		.data(|data| data.get_temp::<Option<(String, f64)>>(egui::Id::unique(COPIED)))
		.flatten()
		.is_some_and(|(copied, at)| copied == key && now - at < COPIED_SECONDS);
	if copied {
		ctx.request_repaint_after(std::time::Duration::from_millis(250));
	}
	copied
}

pub(crate) fn icon(icon: Option<ContributionIcon>) -> crate::icons::Icon {
	icon.map_or(crate::icons::Icon::Sparkle, crate::contributions::icon)
}

/// The link for an attachment, as `media_links` describes it.
pub(crate) fn attachment_link(attachment: &model::Attachment) -> Option<MediaLink> {
	let url = attachment.media.url.clone()?;
	let kind = attachment.content_type.as_deref().unwrap_or("");
	Some(MediaLink {
		source: extensions::MediaSource::Attachment,
		kind: if attachment.is_image() {
			extensions::MediaKind::Image
		} else if kind.starts_with("video/") {
			extensions::MediaKind::Video
		} else if kind.starts_with("audio/") {
			extensions::MediaKind::Audio
		} else {
			extensions::MediaKind::File
		},
		url,
		attachment_id: Some(attachment.id.to_string()),
		filename: Some(attachment.filename.clone()),
		content_type: attachment.content_type.clone(),
		size: Some(attachment.size),
		width: (attachment.media.width > 0).then_some(attachment.media.width),
		height: (attachment.media.height > 0).then_some(attachment.media.height),
	})
}

/// The link for an image embed or any other image shown by URL.
pub(crate) fn image_link(url: &str, width: u32, height: u32) -> Option<MediaLink> {
	url.starts_with("https://").then(|| MediaLink {
		source: extensions::MediaSource::Embed,
		kind: extensions::MediaKind::Image,
		url: url.to_owned(),
		attachment_id: None,
		filename: None,
		content_type: None,
		size: None,
		width: (width > 0).then_some(width),
		height: (height > 0).then_some(height),
	})
}

/// Image-menu entries for every enabled image action: a submenu when it declares choices.
pub(crate) fn image_menu(ui: &mut egui::Ui, link: &MediaLink) {
	let actions = actions(ui.ctx(), Surface::Image);
	if actions.is_empty() {
		return;
	}
	ui.separator();
	for action in actions {
		let pick = |ui: &mut egui::Ui, button: Option<String>| {
			choose(
				ui.ctx(),
				ItemChoice {
					plugin: action.plugin.clone(),
					action: action.action.clone(),
					button,
					media: Some(link.clone()),
					message: None,
				},
			);
			ui.close();
		};
		if action.choices.is_empty() {
			if ui
				.add(crate::icons::menu_item(icon(action.icon), &action.label))
				.clicked()
			{
				pick(ui, None);
			}
		} else {
			ui.menu_button(
				crate::icons::menu_label(icon(action.icon), action.label.clone()),
				|ui| {
					for choice in &action.choices {
						if ui
							.add(crate::icons::menu_item(icon(choice.icon), &choice.label))
							.clicked()
						{
							pick(ui, Some(choice.id.clone()));
						}
					}
				},
			);
		}
	}
}

/// Buttons for attachment actions whose filter matches this file. Returns their responses so
/// the caller can keep them clickable.
pub(crate) fn attachment_buttons(
	ui: &mut egui::Ui,
	attachment: &model::Attachment,
) -> Vec<egui::Response> {
	let mut responses = Vec::new();
	for action in actions(ui.ctx(), Surface::Attachment) {
		if !action.accepts.as_ref().is_some_and(|filter| {
			filter.matches(
				&attachment.filename,
				attachment.content_type.as_deref(),
				attachment.size,
			)
		}) {
			continue;
		}
		let Some(link) = attachment_link(attachment) else {
			continue;
		};
		let key = attachment.id.to_string();
		let copied = copied_recently(ui.ctx(), &key);
		let response = crate::icons::button_immediate(
			ui,
			if copied {
				crate::icons::Icon::Check
			} else {
				icon(action.icon)
			},
			24.0,
			if copied { "Copied!" } else { &action.label },
		);
		if response.clicked() {
			choose(
				ui.ctx(),
				ItemChoice {
					plugin: action.plugin.clone(),
					action: action.action.clone(),
					button: None,
					media: Some(link),
					message: None,
				},
			);
		}
		responses.push(response);
	}
	responses
}

/// Offline previews: draws the image menu with its first submenu open at `at`, using the
/// same entries a right-click shows.
#[cfg(any(test, feature = "demo"))]
pub fn preview_image_menu(ctx: &egui::Context, url: &str, at: egui::Pos2) {
	let Some(link) = image_link(url, 0, 0) else {
		return;
	};
	let menu = |id: &str, pos: egui::Pos2, add: &mut dyn FnMut(&mut egui::Ui)| {
		egui::Area::new(egui::Id::unique(id))
			.order(egui::Order::Foreground)
			.fixed_pos(pos)
			.show(ctx, |ui| {
				egui::Frame::menu(ui.style())
					.show(ui, |ui| {
						ui.set_width(200.0);
						add(ui);
					})
					.response
					.rect
			})
			.inner
	};
	let main = menu("preview-image-menu", at, &mut |ui| {
		for (icon, key) in [
			(
				crate::icons::Icon::Copy,
				"attachments-media-menu-copy-image",
			),
			(
				crate::icons::Icon::Download,
				"attachments-media-menu-save-image-as",
			),
			(crate::icons::Icon::Link, "attachments-media-menu-copy-link"),
		] {
			ui.add(crate::icons::menu_item(icon, crate::i18n::translate(key)));
		}
		image_menu(ui, &link);
	});
	if let Some(action) = actions(ctx, Surface::Image)
		.into_iter()
		.find(|action| !action.choices.is_empty())
	{
		menu(
			"preview-image-submenu",
			egui::pos2(main.right() + 2.0, main.bottom() - 34.0),
			&mut |ui| {
				for choice in &action.choices {
					ui.add(crate::icons::menu_item(icon(choice.icon), &choice.label));
				}
			},
		);
	}
}

#[cfg(test)]
mod tests {
	use crate::extensions_ui::{ExtensionContext, ExtensionEntry};
	use extensions::{Action, AppAction, HostEffect, Invocation, Output, Surface};

	fn entry(surface: Surface) -> ExtensionEntry {
		let manifest = extensions::Manifest {
			origins: Vec::new(),
			api_version: extensions::API_V2,
			id: "probe".into(),
			name: "Probe".into(),
			version: "1.0.0".into(),
			author: "Synthetic".into(),
			license: "MIT".into(),
			source: "https://example.org/probe".into(),
			kind: extensions::ExtensionKind::Plugin,
			capabilities: Vec::new(),
			requires: Default::default(),
			dependencies: Vec::new(),
			conflicts: Vec::new(),
			settings: Vec::new(),
			quotas: Default::default(),
			slots: Vec::new(),
			actions: vec![Action {
				id: "go".into(),
				label: "Go".into(),
				surface,
				icon: None,
				choices: Vec::new(),
				accepts: None,
			}],
		};
		ExtensionEntry {
			description: String::new(),
			preview: None,
			theme_preview: None,
			cover_image: None,
			local_theme: false,
			manifest,
			reviewed: true,
			sha256: "a".repeat(64),
			download_bytes: 1,
			enabled: true,
			cleanup_pending: false,
			update_available: false,
			update_manifest: None,
			runtime: None,
		}
	}

	/// Only Harmony's own buttons skip the result dialog, and only for a copy or links; the
	/// next frame performs the copy.
	#[test]
	fn button_results_that_only_copy_or_open_links_apply_at_once() {
		let state = test_support::demo_state();
		let copy = Output {
			effects: vec![HostEffect::CopyText {
				text: "copied".into(),
			}],
			..Default::default()
		};
		let link = |url: &str| HostEffect::AppAction {
			action: AppAction::OpenExternalLink { url: url.into() },
		};
		let cases = [
			(Surface::Attachment, copy.clone(), true),
			(
				Surface::Image,
				Output {
					effects: vec![link("https://a.example/"), link("https://b.example/")],
					..Default::default()
				},
				true,
			),
			// A message-menu action keeps the confirmation, as does a result with a panel.
			(Surface::Message, copy.clone(), false),
			(
				Surface::MessageToolbar,
				Output {
					panel: vec![extensions::Element::Separator],
					..copy.clone()
				},
				false,
			),
		];
		for (surface, output, direct) in cases {
			let mut view = crate::MessagingUi::default();
			view.extensions.set_entries(vec![entry(surface)]);
			view.extensions.present_output(
				"probe".into(),
				Invocation {
					action: "go".into(),
					..Default::default()
				},
				ExtensionContext::panel(&state),
				output,
				&state,
			);
			assert_eq!(
				view.extensions.direct_effects.is_some(),
				direct,
				"{surface:?}"
			);
			assert_eq!(view.extensions.has_result(), !direct, "{surface:?}");
		}
		let ctx = egui::Context::default();
		let mut view = crate::MessagingUi::default();
		let mut state = test_support::demo_state();
		view.extensions.direct_effects = Some(("7".into(), copy.effects));
		let output = ctx.run_ui(Default::default(), |ui| {
			view.show(ui, &mut state);
		});
		assert!(output.platform_output.commands.iter().any(
			|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "copied")
		));
		output.drop_without_applying_deltas();
		assert!(super::copied_recently(&ctx, "7"));
	}
}
