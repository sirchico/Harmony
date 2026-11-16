//! ClearURLs: removes tracking parameters from links in every message as it is sent. Runs as a
//! `before_send` hook; it only sees outgoing text, changes nothing else, and fetches nothing.
//! Written for Harmony using only the public API v2 SDK.
use harmony_extension_sdk::{ExtensionInvocation, ExtensionOutput};

/// Parameters removed from every link.
const EVERYWHERE: &[&str] = &[
	"fbclid",
	"gclid",
	"gbraid",
	"wbraid",
	"dclid",
	"msclkid",
	"yclid",
	"twclid",
	"ttclid",
	"mc_cid",
	"mc_eid",
	"igshid",
	"igsh",
	"ref_src",
	"ref_url",
	"spm",
	"scm",
	"_hsenc",
	"_hsmi",
	"mkt_tok",
	"s_cid",
	"vero_id",
	"oly_anon_id",
	"oly_enc_id",
	"rb_clickid",
	"__s",
];
/// Parameters removed only for these sites, where they track who shared a link.
const BY_SITE: &[(&[&str], &[&str])] = &[
	(
		&["youtube.com", "youtu.be", "music.youtube.com"],
		&["si", "pp", "feature"],
	),
	(&["open.spotify.com"], &["si", "context"]),
	(&["x.com", "twitter.com"], &["s", "t"]),
	(&["instagram.com"], &["igsh", "img_index"]),
	(
		&["tiktok.com"],
		&["_t", "_r", "is_from_webapp", "sender_device"],
	),
	(&["reddit.com"], &["share_id", "utm_name", "rdt"]),
	(
		&["amazon.com", "amazon.co.uk", "amazon.de"],
		&[
			"ref",
			"ref_",
			"pf_rd_r",
			"pf_rd_p",
			"pd_rd_r",
			"pd_rd_w",
			"pd_rd_wg",
			"content-id",
		],
	),
];

fn host(url: &str) -> &str {
	let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
	let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
	let host = &rest[..end];
	let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
	host.split(':').next().unwrap_or(host)
}

fn tracker(site: &str, key: &str) -> bool {
	let site = site.to_ascii_lowercase();
	key.starts_with("utm_")
		|| EVERYWHERE.contains(&key)
		|| BY_SITE.iter().any(|(sites, keys)| {
			sites
				.iter()
				.any(|known| site == *known || site.ends_with(&format!(".{known}")))
				&& keys.contains(&key)
		})
}

/// One link with tracking parameters removed; everything else is kept exactly as written.
pub fn clean_url(url: &str) -> String {
	let (before_fragment, fragment) = match url.find('#') {
		Some(index) => url.split_at(index),
		None => (url, ""),
	};
	let Some((base, query)) = before_fragment.split_once('?') else {
		return url.to_owned();
	};
	let site = host(base);
	let kept: Vec<&str> = query
		.split('&')
		.filter(|pair| {
			let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
			!pair.is_empty() && !tracker(site, key)
		})
		.collect();
	if kept.is_empty() {
		format!("{base}{fragment}")
	} else {
		format!("{base}?{}{fragment}", kept.join("&"))
	}
}

fn link_end(byte: u8) -> bool {
	byte.is_ascii_whitespace() || matches!(byte, b'<' | b'>' | b'"' | b'\'' | b')' | b']' | b'`')
}

/// Cleans every http(s) link in the text and leaves everything around it untouched.
pub fn clean(text: &str) -> String {
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
		let mut end = tail.bytes().position(link_end).unwrap_or(tail.len());
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

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	let text = input
		.invocation
		.invocation
		.invocation
		.composer
		.clone()
		.unwrap_or_default();
	let cleaned = clean(&text);
	let mut output = ExtensionOutput::default();
	// No answer means "send as typed".
	if cleaned != text {
		output.output.output.replacement = Some(cleaned);
	}
	output
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::testing::TestHost;

	#[test]
	fn trackers_are_removed_site_rules_apply_only_to_their_sites() {
		assert_eq!(
			clean(
				"look https://youtu.be/abc?si=XYZ&t=42 and https://example.com/?si=keep&utm_source=x."
			),
			"look https://youtu.be/abc?t=42 and https://example.com/?si=keep."
		);
		assert_eq!(
			clean("[shop](https://www.amazon.com/dp/B01?ref_=abc&th=1#reviews)"),
			"[shop](https://www.amazon.com/dp/B01?th=1#reviews)"
		);
		assert_eq!(clean("no links, just ?si=text"), "no links, just ?si=text");
	}

	#[test]
	fn the_hook_answers_only_when_something_changed() {
		let send = |text: &str| {
			let mut input = ExtensionInvocation::default();
			input.invocation.invocation.invocation.action = "clean".into();
			input.invocation.invocation.invocation.composer = Some(text.into());
			TestHost::default().run(input, handle).unwrap()
		};
		assert_eq!(
			send("https://x.com/a/status/1?s=20&t=abc")
				.output
				.output
				.replacement
				.as_deref(),
			Some("https://x.com/a/status/1")
		);
		assert!(send("hello").output.output.replacement.is_none());
	}
}
