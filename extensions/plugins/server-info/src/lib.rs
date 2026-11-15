//! Server Info: shows a server's public details from its context menu. Written for Harmony from
//! the plugin's described behavior, using only the public SDK: a `server` menu action, one
//! `server_profile` host request answered between rounds, and an optional `copy_text` effect.
use harmony_extension_sdk::{
	Element, ExtensionInvocation, ExtensionOutput, HostEffect, HostRequest, MenuTarget,
	ResponseValue, ServerProfile,
};

const LOOKUP: &str = "server";

fn line(label: &str, value: impl std::fmt::Display) -> Element {
	Element::Text {
		text: format!("{label}: {value}"),
	}
}

/// `YYYY-MM-DD HH:MM UTC` from Unix milliseconds, without a date library.
pub fn utc(ms: u64) -> String {
	let seconds = ms / 1000;
	let (days, rest) = (seconds / 86_400, seconds % 86_400);
	// Civil-from-days (Howard Hinnant), valid for the Unix era.
	let z = days as i64 + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let day = doy - (153 * mp + 2) / 5 + 1;
	let month = if mp < 10 { mp + 3 } else { mp - 9 };
	let year = yoe + era * 400 + i64::from(month <= 2);
	format!(
		"{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
		rest / 3600,
		rest % 3600 / 60
	)
}

fn level(value: Option<u8>, names: &[&str]) -> Option<String> {
	value.map(|value| {
		names
			.get(usize::from(value))
			.map_or_else(|| format!("Level {value}"), |name| (*name).to_owned())
	})
}

pub fn details(profile: &ServerProfile, features: bool) -> Vec<Element> {
	let mut panel = vec![Element::Heading {
		text: profile.name.clone(),
	}];
	if let Some(description) = &profile.description {
		panel.push(Element::Text {
			text: description.clone(),
		});
	}
	panel.push(Element::Separator);
	panel.push(line("Server ID", &profile.id));
	if let Some(owner) = &profile.owner_id {
		panel.push(line("Owner ID", owner));
	}
	panel.push(line("Created", utc(profile.created_at_ms)));
	if let Some(members) = profile.approximate_member_count {
		let online = profile
			.approximate_presence_count
			.map_or(String::new(), |online| format!(" ({online} online)"));
		panel.push(line("Members", format!("{members}{online}")));
	}
	if let Some(tier) = profile.premium_tier {
		let boosts = profile.premium_subscription_count.unwrap_or(0);
		panel.push(line("Boosts", format!("Level {tier}, {boosts} boosts")));
	}
	if let Some(verification) = level(
		profile.verification_level,
		&["None", "Low", "Medium", "High", "Highest"],
	) {
		panel.push(line("Verification", verification));
	}
	if let Some(filter) = level(
		profile.explicit_content_filter,
		&["Off", "Members without roles", "All members"],
	) {
		panel.push(line("Content filter", filter));
	}
	if let Some(mfa) = level(
		profile.mfa_level,
		&["Not required", "Required for moderators"],
	) {
		panel.push(line("Two-factor for moderation", mfa));
	}
	if let Some(vanity) = &profile.vanity_url_code {
		panel.push(line("Vanity invite", format!("discord.gg/{vanity}")));
	}
	if let Some(locale) = &profile.preferred_locale {
		panel.push(line("Language", locale));
	}
	panel.push(line(
		"Roles · emojis · stickers",
		format!(
			"{} · {} · {}",
			profile.roles, profile.emojis, profile.stickers
		),
	));
	for (label, url) in [
		("Icon", &profile.icon_url),
		("Banner", &profile.banner_url),
		("Invite background", &profile.splash_url),
	] {
		if let Some(url) = url {
			panel.push(line(label, url));
		}
	}
	if features && !profile.features.is_empty() {
		panel.push(Element::Separator);
		panel.push(line("Features", profile.features.join(", ")));
	}
	panel
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let Some(MenuTarget::Server { guild_id }) = &input.menu_target else {
		return ExtensionOutput::panel(vec![Element::Text {
			text: "Open this from a server's menu.".into(),
		}]);
	};
	let Some(response) = input.response(LOOKUP) else {
		let mut output = ExtensionOutput::default();
		output.request(HostRequest::ServerProfile {
			id: LOOKUP.into(),
			guild_id: guild_id.clone(),
		});
		return output;
	};
	match (&response.value, &response.error) {
		(Some(ResponseValue::ServerProfile { profile }), _) => {
			let mut output =
				ExtensionOutput::panel(details(profile, input.setting_bool("show-features")));
			if input.setting_bool("copy-id") {
				output.output.effects.push(HostEffect::CopyText {
					text: profile.id.clone(),
				});
			}
			output
		}
		(_, Some(error)) => ExtensionOutput::panel(vec![
			Element::Heading {
				text: "Server info".into(),
			},
			Element::Text {
				text: format!("Could not load this server: {}", error.message),
			},
		]),
		_ => ExtensionOutput::panel(vec![Element::Text {
			text: "Harmony returned an unexpected answer.".into(),
		}]),
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{SettingValue, testing::TestHost};

	#[test]
	fn the_menu_action_looks_the_server_up_then_shows_it() {
		let mut host = TestHost::default();
		host.servers.insert(
			"10".into(),
			ServerProfile {
				id: "10".into(),
				name: "Synthetic".into(),
				created_at_ms: 1_420_070_400_000,
				approximate_member_count: Some(42),
				features: vec!["COMMUNITY".into()],
				..Default::default()
			},
		);
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "info".into();
		input.menu_target = Some(MenuTarget::Server {
			guild_id: "10".into(),
		});
		input
			.settings
			.insert("show-features".into(), SettingValue::Bool(true));
		let output = host.run(input, handle).unwrap();
		assert_eq!(host.inputs.len(), 2, "one lookup round, then the answer");
		let texts: Vec<_> = output
			.output
			.output
			.panel
			.iter()
			.filter_map(|element| match element {
				Element::Text { text } => Some(text.as_str()),
				_ => None,
			})
			.collect();
		assert!(texts.contains(&"Created: 2015-01-01 00:00 UTC"));
		assert!(texts.contains(&"Features: COMMUNITY"));
		assert!(output.output.effects.is_empty());
	}
}
