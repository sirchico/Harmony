//! Offline Composer Tools: five tools for the draft that run entirely offline. Clear URL
//! trackers, unindent code blocks, convert units, transform text with a /command, and count
//! characters. Written for Harmony using only the public SDK; changes are proposals the user
//! applies, and nothing is sent or fetched.
use harmony_extension_sdk::{Element, ExtensionInvocation, ExtensionOutput};

/// Results larger than this fall back to the unchanged draft.
const MAX_RESULT: usize = 64 * 1024;

// ---- Clear URL trackers ----

const TRACKERS: &[&str] = &[
	"fbclid", "gclid", "dclid", "msclkid", "mc_cid", "mc_eid", "igshid", "ref_src", "spm", "scm",
];

fn tracker(key: &str) -> bool {
	key.starts_with("utm_") || TRACKERS.contains(&key)
}

/// One URL with tracking query parameters removed; everything else is kept as written.
pub fn clean_url(url: &str) -> String {
	let (before_fragment, fragment) = match url.find('#') {
		Some(index) => url.split_at(index),
		None => (url, ""),
	};
	let Some((base, query)) = before_fragment.split_once('?') else {
		return url.to_owned();
	};
	let kept: Vec<&str> = query
		.split('&')
		.filter(|pair| {
			let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
			!pair.is_empty() && !tracker(key)
		})
		.collect();
	if kept.is_empty() {
		format!("{base}{fragment}")
	} else {
		format!("{base}?{}{fragment}", kept.join("&"))
	}
}

fn url_end(byte: u8) -> bool {
	byte.is_ascii_whitespace() || matches!(byte, b'<' | b'>' | b'"' | b'\'' | b')' | b']' | b'`')
}

/// Cleans every http(s) URL in the text, leaving the surrounding text untouched.
pub fn clear_urls(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	loop {
		let next = [rest.find("https://"), rest.find("http://")]
			.into_iter()
			.flatten()
			.min();
		let Some(start) = next else {
			out.push_str(rest);
			return out;
		};
		out.push_str(&rest[..start]);
		let tail = &rest[start..];
		let mut end = tail.bytes().position(url_end).unwrap_or(tail.len());
		// Sentence punctuation right after a link belongs to the sentence.
		while end > 0
			&& matches!(
				tail.as_bytes()[end - 1],
				b'.' | b',' | b'!' | b'?' | b';' | b':'
			) {
			end -= 1;
		}
		out.push_str(&clean_url(&tail[..end]));
		rest = &tail[end..];
	}
}

// ---- Unindent code blocks ----

/// The fence character and length if this line opens or closes a fenced block.
fn fence(line: &str) -> Option<(char, usize)> {
	let trimmed = line.trim_start();
	let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
	let count = trimmed.chars().take_while(|c| *c == marker).count();
	(count >= 3).then_some((marker, count))
}

fn closes(line: &str, marker: char, count: usize) -> bool {
	let trimmed = line.trim();
	trimmed.chars().count() >= count && trimmed.chars().all(|c| c == marker)
}

fn dedent(body: &mut [String]) {
	for line in body.iter_mut() {
		*line = line.replace('\t', "    ");
	}
	let indent = body
		.iter()
		.filter(|line| !line.trim().is_empty())
		.map(|line| line.len() - line.trim_start_matches(' ').len())
		.min()
		.unwrap_or(0);
	for line in body.iter_mut() {
		if !line.trim().is_empty() {
			*line = line[indent..].to_owned();
		}
	}
}

/// Removes the common indentation inside complete fenced code blocks.
pub fn unindent(text: &str) -> String {
	// Keep each line's own ending so CRLF and LF drafts come back as they were.
	let mut lines: Vec<(String, &str)> = Vec::new();
	let mut rest = text;
	while !rest.is_empty() {
		let (line, ending, next) = match rest.find('\n') {
			Some(index) if index > 0 && rest.as_bytes()[index - 1] == b'\r' => {
				(&rest[..index - 1], "\r\n", &rest[index + 1..])
			}
			Some(index) => (&rest[..index], "\n", &rest[index + 1..]),
			None => (rest, "", ""),
		};
		lines.push((line.to_owned(), ending));
		rest = next;
	}
	let mut index = 0;
	while index < lines.len() {
		if let Some((marker, count)) = fence(&lines[index].0)
			&& let Some(close) =
				(index + 1..lines.len()).find(|n| closes(&lines[*n].0, marker, count))
		{
			let mut body: Vec<String> = lines[index + 1..close]
				.iter()
				.map(|(line, _)| line.clone())
				.collect();
			dedent(&mut body);
			for (offset, line) in body.into_iter().enumerate() {
				lines[index + 1 + offset].0 = line;
			}
			index = close + 1;
		} else {
			index += 1;
		}
	}
	lines
		.into_iter()
		.map(|(line, ending)| format!("{line}{ending}"))
		.collect()
}

// ---- Convert units ----

/// (unit, its counterpart, conversion to the counterpart).
/// A unit, its counterpart, and the conversion to the counterpart.
type Unit = (&'static str, &'static str, fn(f64) -> f64);

const UNITS: &[Unit] = &[
	("km", "mi", |v| v / 1.609_344),
	("mi", "km", |v| v * 1.609_344),
	("kg", "lb", |v| v / 0.453_592_37),
	("lb", "kg", |v| v * 0.453_592_37),
	("cm", "in", |v| v / 2.54),
	("in", "cm", |v| v * 2.54),
	("ft", "m", |v| v * 0.3048),
	("m", "ft", |v| v / 0.3048),
	("C", "F", |v| v * 9.0 / 5.0 + 32.0),
	("F", "C", |v| (v - 32.0) * 5.0 / 9.0),
];

fn format_number(value: f64) -> String {
	let text = format!("{value:.2}");
	let text = text.trim_end_matches('0').trim_end_matches('.');
	if text == "-0" {
		"0".into()
	} else {
		text.into()
	}
}

/// Length of a number at the start of `text`: sign, digits, optional fraction and exponent.
fn number_len(text: &str) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
	let digits = |from: usize| {
		bytes[from..]
			.iter()
			.take_while(|b| b.is_ascii_digit())
			.count()
	};
	let whole = digits(index);
	index += whole;
	let mut fraction = 0;
	if bytes.get(index) == Some(&b'.') {
		fraction = digits(index + 1);
		if fraction > 0 {
			index += 1 + fraction;
		}
	}
	if whole + fraction == 0 {
		return None;
	}
	if matches!(bytes.get(index), Some(b'e' | b'E')) {
		let sign = usize::from(matches!(bytes.get(index + 1), Some(b'+' | b'-')));
		let exponent = digits(index + 1 + sign);
		if exponent > 0 {
			index += 1 + sign + exponent;
		}
	}
	Some(index)
}

fn word_byte(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Replaces each measurement such as `10 km` or `20°C` with the counterpart unit.
pub fn convert_units(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	let mut index = 0;
	let bytes = text.as_bytes();
	while index < text.len() {
		let starts_number = (bytes[index].is_ascii_digit()
			|| matches!(bytes[index], b'+' | b'-' | b'.'))
			&& (index == 0 || !(word_byte(bytes[index - 1]) || bytes[index - 1] == b'.'));
		if starts_number && let Some(length) = number_len(&text[index..]) {
			let number = &text[index..index + length];
			let after = &text[index + length..];
			let gap = after.len() - after.trim_start_matches([' ', '\t']).len();
			let unit_text = &after[gap..];
			let degree = unit_text.starts_with('°');
			let unit_text = unit_text.strip_prefix('°').unwrap_or(unit_text);
			let found = UNITS.iter().find(|(unit, ..)| {
				unit_text.starts_with(unit)
					&& !unit_text
						.as_bytes()
						.get(unit.len())
						.is_some_and(|b| word_byte(*b))
					&& (!degree || matches!(*unit, "C" | "F"))
			});
			if let Some((unit, other, convert)) = found
				&& let Ok(value) = number.parse::<f64>()
				&& let converted = convert(value)
				&& converted.is_finite()
			{
				out.push_str(&format_number(converted));
				out.push_str(&after[..gap]);
				if degree {
					out.push('°');
				}
				out.push_str(other);
				index += length + gap + usize::from(degree) * '°'.len_utf8() + unit.len();
				continue;
			}
		}
		let ch = text[index..].chars().next().unwrap_or(' ');
		out.push(ch);
		index += ch.len_utf8();
	}
	out
}

// ---- Transform text ----

const MORSE: &[(char, &str)] = &[
	('a', ".-"),
	('b', "-..."),
	('c', "-.-."),
	('d', "-.."),
	('e', "."),
	('f', "..-."),
	('g', "--."),
	('h', "...."),
	('i', ".."),
	('j', ".---"),
	('k', "-.-"),
	('l', ".-.."),
	('m', "--"),
	('n', "-."),
	('o', "---"),
	('p', ".--."),
	('q', "--.-"),
	('r', ".-."),
	('s', "..."),
	('t', "-"),
	('u', "..-"),
	('v', "...-"),
	('w', ".--"),
	('x', "-..-"),
	('y', "-.--"),
	('z', "--.."),
	('0', "-----"),
	('1', ".----"),
	('2', "..---"),
	('3', "...--"),
	('4', "....-"),
	('5', "....."),
	('6', "-...."),
	('7', "--..."),
	('8', "---.."),
	('9', "----."),
	('.', ".-.-.-"),
	(',', "--..--"),
	('?', "..--.."),
	('\'', ".----."),
	('!', "-.-.--"),
	('/', "-..-."),
	('(', "-.--."),
	(')', "-.--.-"),
	('&', ".-..."),
	(':', "---..."),
	(';', "-.-.-."),
	('=', "-...-"),
	('+', ".-.-."),
	('-', "-....-"),
	('_', "..--.-"),
	('"', ".-..-."),
	('$', "...-..-"),
	('@', ".--.-."),
];

fn morse(text: &str) -> String {
	text.split_whitespace()
		.map(|word| {
			word.chars()
				.map(|c| {
					MORSE
						.iter()
						.find(|(letter, _)| *letter == c.to_ascii_lowercase())
						.map_or_else(|| c.to_string(), |(_, code)| (*code).to_owned())
				})
				.collect::<Vec<_>>()
				.join(" ")
		})
		.collect::<Vec<_>>()
		.join(" / ")
}

fn mock(text: &str) -> String {
	let mut upper = false;
	let mut out = String::with_capacity(text.len());
	for c in text.chars() {
		if c.is_lowercase() || c.is_uppercase() {
			if upper {
				out.extend(c.to_uppercase());
			} else {
				out.extend(c.to_lowercase());
			}
			upper = !upper;
		} else {
			out.push(c);
		}
	}
	out
}

/// Applies `/mock`, `/reverse`, `/morse` or `/wordcount` when the draft starts with exactly
/// that command; anything else comes back unchanged.
pub fn transform(text: &str) -> String {
	for (command, apply) in [
		("/mock", mock as fn(&str) -> String),
		("/reverse", |body: &str| body.chars().rev().collect()),
		("/morse", morse),
		("/wordcount", |body: &str| {
			body.split_whitespace().count().to_string()
		}),
	] {
		if let Some(rest) = text.strip_prefix(command)
			&& (rest.is_empty() || rest.starts_with(char::is_whitespace))
		{
			return apply(rest.trim_start());
		}
	}
	text.to_owned()
}

// ---- Handler ----

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let draft = input
		.invocation
		.invocation
		.invocation
		.composer
		.clone()
		.unwrap_or_default();
	let changed = match input.action() {
		"clear-urls" => clear_urls(&draft),
		"unindent" => unindent(&draft),
		"convert-units" => convert_units(&draft),
		"transform-text" => transform(&draft),
		_ => {
			return ExtensionOutput::panel(vec![
				Element::Heading {
					text: "Character count".into(),
				},
				Element::Text {
					text: format!("Characters: {}", draft.chars().count()),
				},
				Element::Text {
					text: format!("UTF-8 bytes: {}", draft.len()),
				},
				Element::Text {
					text: format!("UTF-16 units: {}", draft.encode_utf16().count()),
				},
				Element::Text {
					text: format!("Words: {}", draft.split_whitespace().count()),
				},
			]);
		}
	};
	let mut output = ExtensionOutput::default();
	output.output.output.replacement = Some(if changed.len() > MAX_RESULT {
		draft
	} else {
		changed
	});
	output
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn trackers_are_removed_and_everything_else_is_kept() {
		assert_eq!(
			clear_urls(
				"see [x](https://a.com/p?utm_source=x&id=7&fbclid=1#top) and http://b.org/?gclid=2."
			),
			"see [x](https://a.com/p?id=7#top) and http://b.org/."
		);
		assert_eq!(clear_urls("no links here"), "no links here");
		assert_eq!(
			clean_url("https://a.com/?q=a%26b"),
			"https://a.com/?q=a%26b"
		);
	}

	#[test]
	fn code_blocks_are_dedented_and_line_endings_kept() {
		assert_eq!(
			unindent("text\r\n```rs\r\n    fn a() {\r\n\t    b();\r\n    }\r\n```\r\n  prose"),
			"text\r\n```rs\r\nfn a() {\r\n    b();\r\n}\r\n```\r\n  prose"
		);
		assert_eq!(unindent("```\n  open fence"), "```\n  open fence");
	}

	#[test]
	fn measurements_convert_and_other_text_does_not() {
		assert_eq!(convert_units("10 km and 20°C"), "6.21 mi and 68°F");
		assert_eq!(
			convert_units("12in, -40F, 1.5e1kg"),
			"30.48cm, -40C, 33.07lb"
		);
		assert_eq!(convert_units("v2km x10m km3 kmh"), "v2km x10m km3 kmh");
	}

	#[test]
	fn commands_must_match_exactly() {
		assert_eq!(transform("/mock hello"), "hElLo");
		assert_eq!(transform("/reverse abc"), "cba");
		assert_eq!(transform("/morse SOS hi"), "... --- ... / .... ..");
		assert_eq!(transform("/wordcount a b  c"), "3");
		for unchanged in ["/mocking x", "/Mock x", "hello /mock"] {
			assert_eq!(transform(unchanged), unchanged);
		}
	}
}
