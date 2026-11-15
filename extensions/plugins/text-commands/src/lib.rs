//! Text Commands: literal text replacement rules and reusable message tags for the draft.
//! Rules and tags are edited in a panel and stored locally; applying them to the draft is a
//! proposal the user applies. Written for Harmony using only the public SDK.
use harmony_extension_sdk::{Element, Invocation, MAX_IO_BYTES, Output, serde_json};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_RULES: usize = 8;
const MAX_TAGS: usize = 8;
const MAX_FIELD: usize = 4096;
const MAX_STORAGE: usize = 64 * 1024;
const MAX_DRAFT: usize = 8000;
const MAX_ARGUMENTS: usize = 64;

/// Saved data; field names match what earlier versions stored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Saved {
	pub rules: Vec<Rule>,
	pub tags: Vec<Tag>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
	pub find: String,
	pub replace: String,
	#[serde(default)]
	pub only_if_includes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tag {
	pub name: String,
	pub message: String,
}

/// Saved data that could not be read; it is left untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable;

/// A tag name: a lowercase letter, then lowercase letters, digits or hyphens; 1 to 32 bytes.
pub fn valid_name(name: &str) -> bool {
	name.len() <= 32
		&& name.starts_with(|c: char| c.is_ascii_lowercase())
		&& name
			.bytes()
			.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Saved {
	fn valid(&self) -> bool {
		let mut finds = std::collections::BTreeSet::new();
		let mut names = std::collections::BTreeSet::new();
		self.rules.len() <= MAX_RULES
			&& self.tags.len() <= MAX_TAGS
			&& self.rules.iter().all(|rule| {
				!rule.find.is_empty()
					&& [&rule.find, &rule.replace, &rule.only_if_includes]
						.iter()
						.all(|field| field.len() <= MAX_FIELD)
					&& finds.insert(rule.find.as_str())
			}) && self.tags.iter().all(|tag| {
			valid_name(&tag.name)
				&& !tag.message.trim().is_empty()
				&& tag.message.len() <= MAX_FIELD
				&& names.insert(tag.name.as_str())
		})
	}
	/// The saved data, `Ok(default)` when nothing is saved, or `Err` when it is unreadable.
	pub fn load(storage: Option<&str>) -> Result<Self, Unreadable> {
		let Some(text) = storage.filter(|text| !text.is_empty()) else {
			return Ok(Self::default());
		};
		if text.len() > MAX_STORAGE {
			return Err(Unreadable);
		}
		serde_json::from_str::<Self>(text)
			.ok()
			.filter(Self::valid)
			.ok_or(Unreadable)
	}
}

fn unescape(text: &str) -> String {
	text.replace("\\n", "\n")
}

/// Runs every rule once, in order, on the result of the rules before it.
pub fn apply_rules(rules: &[Rule], draft: &str) -> Result<String, &'static str> {
	let mut text = draft.to_owned();
	for rule in rules {
		if !rule.only_if_includes.is_empty() && !text.contains(&rule.only_if_includes) {
			continue;
		}
		text = text.replace(&rule.find, &unescape(&rule.replace));
		if text.len() > MAX_DRAFT {
			return Err("The replacements would make the draft longer than 8,000 bytes.");
		}
	}
	Ok(text)
}

/// Placeholders in order of first appearance: (name, default).
fn placeholders(template: &str) -> Vec<(String, Option<String>)> {
	let mut found: Vec<(String, Option<String>)> = Vec::new();
	let mut rest = template;
	while let Some(start) = rest.find("{{") {
		let Some(end) = rest[start + 2..].find("}}") else {
			break;
		};
		let inside = &rest[start + 2..start + 2 + end];
		let (name, default) = match inside.split_once('=') {
			Some((name, default)) => (name.trim(), Some(default.trim().to_owned())),
			None => (inside.trim(), None),
		};
		if !name.is_empty()
			&& !found.iter().any(|(known, _)| known == name)
			&& found.len() < MAX_ARGUMENTS
		{
			found.push((name.to_owned(), default));
		}
		rest = &rest[start + 2 + end + 2..];
	}
	found
}

/// Splits `key:value key2:value two` into values, starting a new value only at a space
/// followed by another placeholder name that has not been supplied yet.
fn arguments(text: &str, names: &[String]) -> Result<BTreeMap<String, String>, String> {
	let mut supplied = BTreeMap::new();
	let mut rest = text.trim();
	if rest.is_empty() {
		return Ok(supplied);
	}
	let starts = |text: &str, supplied: &BTreeMap<String, String>| {
		names
			.iter()
			.filter(|name| !supplied.contains_key(*name))
			.find(|name| text.starts_with(&format!("{name}:")))
			.cloned()
	};
	let Some(mut current) = starts(rest, &supplied) else {
		return Err(format!(
			"Unknown argument: {}",
			rest.split(':').next().unwrap_or(rest)
		));
	};
	rest = &rest[current.len() + 1..];
	loop {
		let mut split = None;
		for (index, _) in rest.match_indices(' ') {
			let mut pending = supplied.clone();
			pending.insert(current.clone(), String::new());
			if let Some(next) = starts(&rest[index + 1..], &pending) {
				split = Some((index, next));
				break;
			}
		}
		match split {
			Some((index, next)) => {
				supplied.insert(current, rest[..index].trim().to_owned());
				rest = &rest[index + 1 + next.len() + 1..];
				current = next;
			}
			None => {
				supplied.insert(current, rest.trim().to_owned());
				return Ok(supplied);
			}
		}
	}
}

/// Expands `/name args` with the named tag; errors explain why nothing changed.
pub fn expand(tags: &[Tag], draft: &str) -> Result<String, String> {
	let Some(command) = draft.strip_prefix('/') else {
		return Err("Start the draft with /name to use a tag.".into());
	};
	let (name, rest) = command
		.split_once(char::is_whitespace)
		.unwrap_or((command, ""));
	let tag = tags
		.iter()
		.find(|tag| tag.name == name)
		.ok_or_else(|| format!("There is no tag named {name}."))?;
	let wanted = placeholders(&tag.message);
	let names: Vec<String> = wanted.iter().map(|(name, _)| name.clone()).collect();
	let supplied = arguments(rest, &names)?;
	let mut text = tag.message.clone();
	for (name, default) in &wanted {
		let value = supplied
			.get(name)
			.cloned()
			.or_else(|| default.clone())
			.ok_or_else(|| format!("Missing argument: {name}"))?;
		let mut out = String::with_capacity(text.len());
		let mut rest = text.as_str();
		while let Some(start) = rest.find("{{") {
			let Some(end) = rest[start + 2..].find("}}") else {
				break;
			};
			let inside = &rest[start + 2..start + 2 + end];
			let placeholder = inside
				.split_once('=')
				.map_or(inside, |(name, _)| name)
				.trim();
			out.push_str(&rest[..start]);
			if placeholder == name {
				out.push_str(&value);
			} else {
				out.push_str(&rest[start..start + 2 + end + 2]);
			}
			rest = &rest[start + 2 + end + 2..];
		}
		out.push_str(rest);
		text = out;
	}
	let text = unescape(&text);
	if text.trim().is_empty() {
		return Err("The tag expanded to nothing.".into());
	}
	if text.len() > MAX_DRAFT {
		return Err("The tag would make the draft longer than 8,000 bytes.".into());
	}
	Ok(text)
}

fn input_field(id: &str, label: &str, value: &str) -> Element {
	Element::TextInput {
		id: id.into(),
		label: label.into(),
		value: value.into(),
	}
}

fn button(id: &str, label: &str) -> Element {
	Element::Button {
		id: id.into(),
		label: label.into(),
	}
}

/// The selected entry when it still exists, otherwise the first one.
fn chosen(options: &[String], selected: String) -> String {
	if options.contains(&selected) {
		selected
	} else {
		options.first().cloned().unwrap_or_default()
	}
}

fn editor(saved: &Saved, fields: &BTreeMap<&str, String>, note: Option<&str>) -> Vec<Element> {
	let field = |id: &str| fields.get(id).cloned().unwrap_or_default();
	let mut panel = vec![Element::Heading {
		text: "Text replacements".into(),
	}];
	if let Some(note) = note {
		panel.push(Element::Text { text: note.into() });
	}
	panel.extend([
		input_field("find", "Find", &field("find")),
		input_field(
			"replace",
			"Replace with (\\n for a new line)",
			&field("replace"),
		),
		input_field(
			"condition",
			"Only if the draft includes",
			&field("condition"),
		),
	]);
	if !saved.rules.is_empty() {
		let options: Vec<String> = saved.rules.iter().map(|rule| rule.find.clone()).collect();
		panel.push(Element::Select {
			id: "rule-selection".into(),
			label: "Saved rules".into(),
			value: chosen(&options, field("rule-selection")),
			options,
		});
	}
	panel.push(Element::Row {
		children: vec![
			button("save-rule", "Save rule"),
			button("load-rule", "Edit selected"),
			button("delete-rule", "Delete rule"),
		],
	});
	panel.push(Element::Separator);
	panel.push(Element::Heading {
		text: "Tags".into(),
	});
	panel.extend([
		input_field("name", "Name (use /name in the composer)", &field("name")),
		input_field(
			"message",
			"Message ({{arg}} or {{arg = default}})",
			&field("message"),
		),
	]);
	if !saved.tags.is_empty() {
		let options: Vec<String> = saved.tags.iter().map(|tag| tag.name.clone()).collect();
		panel.push(Element::Select {
			id: "tag-selection".into(),
			label: "Saved tags".into(),
			value: chosen(&options, field("tag-selection")),
			options,
		});
	}
	panel.push(Element::Row {
		children: vec![
			button("save-tag", "Save tag"),
			button("load-tag", "Edit selected"),
			button("delete-tag", "Delete tag"),
		],
	});
	panel
}

fn panel(elements: Vec<Element>) -> Output {
	Output {
		panel: elements,
		..Default::default()
	}
}

const UNREADABLE: &str =
	"Saved rules and tags could not be read, so nothing can be changed. They are left as they are.";

fn handle(input: Invocation) -> Output {
	let saved = Saved::load(input.storage.as_deref());
	let mut fields: BTreeMap<&str, String> = [
		"find",
		"replace",
		"condition",
		"name",
		"message",
		"rule-selection",
		"tag-selection",
	]
	.into_iter()
	.map(|id| (id, input.value(id).unwrap_or_default().to_owned()))
	.collect();
	let draft = input.composer.clone().unwrap_or_default();
	match (input.action.as_str(), saved) {
		("apply-replacements", Ok(saved)) => match apply_rules(&saved.rules, &draft) {
			Ok(text) => Output {
				replacement: Some(text),
				..Default::default()
			},
			Err(reason) => panel(vec![Element::Text {
				text: reason.into(),
			}]),
		},
		("expand-tag", Ok(saved)) => match expand(&saved.tags, &draft) {
			Ok(text) => Output {
				replacement: Some(text),
				..Default::default()
			},
			Err(reason) => panel(vec![Element::Text { text: reason }]),
		},
		(_, Err(Unreadable)) => panel(vec![Element::Text {
			text: UNREADABLE.into(),
		}]),
		(action, Ok(mut saved)) => {
			let note = match action {
				"save-rule" => {
					let rule = Rule {
						find: fields["find"].clone(),
						replace: fields["replace"].clone(),
						only_if_includes: fields["condition"].clone(),
					};
					if rule.find.is_empty() {
						Some("Type the text to find first.")
					} else if [&rule.find, &rule.replace, &rule.only_if_includes]
						.iter()
						.any(|field| field.len() > MAX_FIELD)
					{
						Some("Each field holds at most 4,096 bytes.")
					} else if let Some(existing) =
						saved.rules.iter_mut().find(|old| old.find == rule.find)
					{
						*existing = rule;
						Some("Rule updated.")
					} else if saved.rules.len() >= MAX_RULES {
						Some("You can keep up to 8 rules.")
					} else {
						saved.rules.push(rule);
						Some("Rule saved.")
					}
				}
				"delete-rule" => {
					saved.rules.retain(|rule| rule.find != fields["find"]);
					Some("Rule deleted if it existed.")
				}
				"load-rule" => {
					if let Some(rule) = saved
						.rules
						.iter()
						.find(|rule| rule.find == fields["rule-selection"])
					{
						fields.insert("find", rule.find.clone());
						fields.insert("replace", rule.replace.clone());
						fields.insert("condition", rule.only_if_includes.clone());
					}
					None
				}
				"save-tag" => {
					let tag = Tag {
						name: fields["name"].trim().to_owned(),
						message: fields["message"].clone(),
					};
					if !valid_name(&tag.name) {
						Some(
							"Names start with a lowercase letter and use lowercase letters, digits and hyphens (up to 32).",
						)
					} else if tag.message.trim().is_empty() || tag.message.len() > MAX_FIELD {
						Some("A tag needs a message of at most 4,096 bytes.")
					} else if let Some(existing) =
						saved.tags.iter_mut().find(|old| old.name == tag.name)
					{
						*existing = tag;
						Some("Tag updated.")
					} else if saved.tags.len() >= MAX_TAGS {
						Some("You can keep up to 8 tags.")
					} else {
						saved.tags.push(tag);
						Some("Tag saved.")
					}
				}
				"delete-tag" => {
					saved.tags.retain(|tag| tag.name != fields["name"].trim());
					Some("Tag deleted if it existed.")
				}
				"load-tag" => {
					if let Some(tag) = saved
						.tags
						.iter()
						.find(|tag| tag.name == fields["tag-selection"])
					{
						fields.insert("name", tag.name.clone());
						fields.insert("message", tag.message.clone());
					}
					None
				}
				_ => None,
			};
			let mut output = panel(editor(&saved, &fields, note));
			if matches!(
				action,
				"save-rule" | "delete-rule" | "save-tag" | "delete-tag"
			) {
				match serde_json::to_string(&saved) {
					Ok(text) if text.len() <= MAX_STORAGE && text.len() < MAX_IO_BYTES / 2 => {
						output.storage = Some(text);
					}
					_ => {
						output = panel(editor(&saved, &fields, Some("Too much to save.")));
					}
				}
			}
			output
		}
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;

	fn rule(find: &str, replace: &str, condition: &str) -> Rule {
		Rule {
			find: find.into(),
			replace: replace.into(),
			only_if_includes: condition.into(),
		}
	}

	#[test]
	fn rules_run_once_in_order_with_conditions_on_the_running_text() {
		let rules = [
			rule("brb", "be right back", ""),
			rule("back", "BACK", "right"),
			rule("x", "line\\nbreak", "never"),
		];
		assert_eq!(apply_rules(&rules, "brb x").unwrap(), "be right BACK x");
		assert!(apply_rules(&[rule("a", &"b".repeat(9000), "")], "a").is_err());
	}

	#[test]
	fn tags_expand_arguments_defaults_and_report_problems() {
		let tags = [Tag {
			name: "greet".into(),
			message: "Hi {{first name}}, from {{who = me}}\\nbye".into(),
		}];
		assert_eq!(
			expand(&tags, "/greet first name:Ada Lovelace who:Bob").unwrap(),
			"Hi Ada Lovelace, from Bob\nbye"
		);
		assert_eq!(
			expand(&tags, "/greet first name:Ada").unwrap(),
			"Hi Ada, from me\nbye"
		);
		assert!(expand(&tags, "/greet").unwrap_err().contains("first name"));
		assert!(expand(&tags, "/greet age:3").is_err());
		assert!(expand(&tags, "/nope").is_err());
	}

	#[test]
	fn earlier_saved_data_loads_and_unreadable_data_blocks_changes() {
		let earlier = r#"{"rules":[{"find":"a","replace":"b","only_if_includes":""}],"tags":[{"name":"t","message":"m"}]}"#;
		assert_eq!(Saved::load(Some(earlier)).unwrap().rules.len(), 1);
		for broken in [
			r#"{"rules":5}"#,
			r#"{"extra":1}"#,
			r#"{"tags":[{"name":"Bad","message":"m"}]}"#,
		] {
			assert!(Saved::load(Some(broken)).is_err());
		}
		let blocked = handle(Invocation {
			action: "save-rule".into(),
			storage: Some("{oops".into()),
			..Default::default()
		});
		assert!(blocked.storage.is_none());
	}
}
