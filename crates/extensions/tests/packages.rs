//! Every published plugin package must answer each of its actions with output the host
//! accepts. Plugin unit tests use the SDK's lenient types, so this is where an invalid panel
//! (an ID the host refuses, a selection outside its options) is caught before release.
use extensions::{Invocation, MediaKind, MediaLink, MediaSource, SelectedMessageRef, Surface};

const PACKAGES: &[(&str, &[u8])] = &[
	(
		"api-proxy",
		include_bytes!("../../../extensions/plugins/packages/api-proxy.harmony-extension"),
	),
	(
		"base-decoder",
		include_bytes!("../../../extensions/plugins/packages/base-decoder.harmony-extension"),
	),
	(
		"copy-file-contents",
		include_bytes!("../../../extensions/plugins/packages/copy-file-contents.harmony-extension"),
	),
	(
		"custom-rpc",
		include_bytes!("../../../extensions/plugins/packages/custom-rpc.harmony-extension"),
	),
	(
		"external-search-tools",
		include_bytes!(
			"../../../extensions/plugins/packages/external-search-tools.harmony-extension"
		),
	),
	(
		"message-delete-protector",
		include_bytes!(
			"../../../extensions/plugins/packages/message-delete-protector.harmony-extension"
		),
	),
	(
		"offline-composer-tools",
		include_bytes!(
			"../../../extensions/plugins/packages/offline-composer-tools.harmony-extension"
		),
	),
	(
		"reverse-image-search",
		include_bytes!(
			"../../../extensions/plugins/packages/reverse-image-search.harmony-extension"
		),
	),
	(
		"server-info",
		include_bytes!("../../../extensions/plugins/packages/server-info.harmony-extension"),
	),
	(
		"text-commands",
		include_bytes!("../../../extensions/plugins/packages/text-commands.harmony-extension"),
	),
	(
		"user-tags",
		include_bytes!("../../../extensions/plugins/packages/user-tags.harmony-extension"),
	),
	(
		"view-raw-message",
		include_bytes!("../../../extensions/plugins/packages/view-raw-message.harmony-extension"),
	),
];

fn link(kind: MediaKind, name: &str) -> MediaLink {
	MediaLink {
		source: MediaSource::Attachment,
		kind,
		url: format!("https://cdn.discordapp.com/attachments/1/2/{name}"),
		attachment_id: Some("2".into()),
		filename: Some(name.into()),
		content_type: None,
		size: Some(5),
		width: None,
		height: None,
	}
}

/// The smallest input each surface receives in the app.
fn input(action: &str, surface: Surface) -> Option<Invocation> {
	let mut invocation = Invocation {
		action: action.into(),
		..Default::default()
	};
	match surface {
		Surface::Panel | Surface::Activation => {}
		Surface::Message | Surface::MessageToolbar => {
			invocation.selected_message = Some("Synthetic aGVsbG8gd29ybGQ= message".into());
			invocation.selected_message_ref = Some(Box::new(SelectedMessageRef {
				channel_id: "2".into(),
				message_id: "3".into(),
				author_id: "4".into(),
				guild_id: Some("5".into()),
			}));
		}
		Surface::Composer => invocation.composer = Some("/mock hello 10 km brb".into()),
		Surface::Image => invocation.media = Some(vec![link(MediaKind::Image, "a.png")]),
		Surface::Attachment => invocation.media = Some(vec![link(MediaKind::File, "a.txt")]),
		Surface::User => {
			invocation.menu_target = Some(Box::new(extensions::MenuTarget::User {
				user_id: "4".into(),
				guild_id: None,
				channel_id: None,
			}))
		}
		Surface::Server => {
			invocation.menu_target = Some(Box::new(extensions::MenuTarget::Server {
				guild_id: "5".into(),
			}))
		}
		// Events and ticks need host data these packages do not subscribe to.
		_ => return None,
	}
	Some(invocation)
}

#[test]
fn every_published_plugin_action_returns_output_the_host_accepts() {
	for (name, bytes) in PACKAGES {
		let package =
			extensions::parse_package(bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
		assert_eq!(package.manifest.id, *name);
		for action in &package.manifest.actions {
			let Some(invocation) = input(&action.id, action.surface) else {
				continue;
			};
			let mut buttons = vec![None];
			buttons.extend(action.choices.iter().map(|choice| Some(choice.id.clone())));
			for button in buttons {
				let mut invocation = invocation.clone();
				invocation.button = button.clone().filter(|_| package.manifest.api_version >= 2);
				if let Err(error) = extensions::invoke(&package, &invocation) {
					panic!("{name} / {} / {button:?}: {error}", action.id);
				}
			}
		}
	}
}
