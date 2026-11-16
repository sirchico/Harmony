//! GIF Provider Switcher: choose where the GIF picker searches (Tenor, Giphy, Klipy, or all
//! three) from a dropdown beside its search field. Klipy is Harmony's built-in search; this
//! extension answers for Tenor and Giphy. API keys stay secrets that Harmony adds to the
//! request, so this code never sees them. Written for Harmony using only the public API v2 SDK:
//! a `gif_search` action and `http` requests.
use harmony_extension_sdk::{
	Element, ExtensionInvocation, ExtensionOutput, GifResult, HostRequest, HttpRequest,
	MAX_GIF_RESULTS, ResponseValue, serde_json,
};
use serde::Deserialize;

/// GIFs asked for per provider and search.
const LIMIT: usize = 40;

fn encode(text: &str) -> String {
	let mut out = String::with_capacity(text.len() * 3);
	for byte in text.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
			out.push(byte as char);
		} else {
			out.push_str(&format!("%{byte:02X}"));
		}
	}
	out
}

/// The request for one search; an empty query asks for trending GIFs.
pub fn request(provider: &str, rating: &str, query: &str) -> HttpRequest {
	let query = query.trim();
	if provider == "tenor" {
		// Tenor's filters: high = G, medium = PG/PG-13, low = R.
		let filter = match rating {
			"g" => "high",
			"r" => "low",
			_ => "medium",
		};
		let endpoint = if query.is_empty() {
			"featured?".to_owned()
		} else {
			format!("search?q={}&", encode(query))
		};
		HttpRequest::get(format!(
			"https://tenor.googleapis.com/v2/{endpoint}limit={LIMIT}&media_filter=gif,tinygif&contentfilter={filter}"
		))
		.secret_param("key", "tenor-key")
	} else {
		let endpoint = if query.is_empty() {
			"trending?".to_owned()
		} else {
			format!("search?q={}&", encode(query))
		};
		HttpRequest::get(format!(
			"https://api.giphy.com/v1/gifs/{endpoint}limit={LIMIT}&rating={rating}"
		))
		.secret_param("api_key", "giphy-key")
	}
}

#[derive(Deserialize)]
struct TenorPage {
	#[serde(default)]
	results: Vec<TenorGif>,
}
#[derive(Deserialize)]
struct TenorGif {
	id: String,
	#[serde(default)]
	content_description: String,
	#[serde(default)]
	itemurl: String,
	media_formats: TenorFormats,
}
#[derive(Deserialize)]
struct TenorFormats {
	#[serde(default)]
	tinygif: Option<TenorMedia>,
	#[serde(default)]
	gif: Option<TenorMedia>,
}
#[derive(Deserialize)]
struct TenorMedia {
	url: String,
	#[serde(default)]
	dims: Vec<u32>,
}

#[derive(Deserialize)]
struct GiphyPage {
	#[serde(default)]
	data: Vec<GiphyGif>,
}
#[derive(Deserialize)]
struct GiphyGif {
	id: String,
	#[serde(default)]
	title: String,
	#[serde(default)]
	url: String,
	images: GiphyImages,
}
#[derive(Deserialize)]
struct GiphyImages {
	fixed_width: Option<GiphyImage>,
}
#[derive(Deserialize)]
struct GiphyImage {
	url: String,
	#[serde(default)]
	width: String,
	#[serde(default)]
	height: String,
}

/// The address without its query string; picker previews may not carry one.
fn plain(url: &str) -> String {
	url.split(['?', '#']).next().unwrap_or_default().to_owned()
}

fn short(text: &str, chars: usize) -> String {
	text.chars().take(chars).collect()
}

/// GIFs from a provider's answer, with IDs prefixed by the provider (`tenor-…`, `giphy-…`) so
/// the picker can badge them and results from several providers never collide.
pub fn parse(provider: &str, body: &str, limit: usize) -> Option<Vec<GifResult>> {
	let gifs: Vec<GifResult> = if provider == "tenor" {
		let page: TenorPage = serde_json::from_str(body).ok()?;
		page.results
			.into_iter()
			.filter_map(|gif| {
				let media = gif.media_formats.tinygif.or(gif.media_formats.gif)?;
				let (width, height) = match media.dims.as_slice() {
					[width, height, ..] => (*width, *height),
					_ => (220, 220),
				};
				let url = if gif.itemurl.starts_with("https://") {
					gif.itemurl
				} else {
					media.url.clone()
				};
				Some(GifResult {
					id: format!("tenor-{}", short(&gif.id, 50)),
					title: short(&gif.content_description, 200),
					url,
					preview: plain(&media.url),
					width,
					height,
				})
			})
			.collect()
	} else {
		let page: GiphyPage = serde_json::from_str(body).ok()?;
		page.data
			.into_iter()
			.filter_map(|gif| {
				let image = gif.images.fixed_width?;
				let url = if gif.url.starts_with("https://") {
					gif.url
				} else {
					format!("https://media.giphy.com/media/{}/giphy.gif", gif.id)
				};
				Some(GifResult {
					id: format!("giphy-{}", short(&gif.id, 50)),
					title: short(&gif.title, 200),
					url,
					preview: plain(&image.url),
					width: image.width.parse().unwrap_or(200),
					height: image.height.parse().unwrap_or(200),
				})
			})
			.collect()
	};
	Some(
		gifs.into_iter()
			.filter(|gif| gif.id.len() > 6 && gif.width > 0 && gif.height > 0)
			.filter(|gif| gif.width <= 4096 && gif.height <= 4096)
			.take(limit)
			.collect(),
	)
}

fn problem(text: impl Into<String>) -> ExtensionOutput {
	ExtensionOutput::panel(vec![Element::Text { text: text.into() }])
}

fn name(provider: &str) -> &'static str {
	if provider == "tenor" {
		"Tenor"
	} else {
		"Giphy"
	}
}

/// One provider's results, or why there are none.
fn answer(
	input: &ExtensionInvocation,
	provider: &str,
	limit: usize,
) -> Result<Vec<GifResult>, String> {
	let name = name(provider);
	let Some(response) = input.response(provider) else {
		return Err(format!("{name} did not answer."));
	};
	match (&response.value, &response.error) {
		(Some(ResponseValue::Http { response }), _) if response.status == 200 => {
			parse(provider, &response.body, limit)
				.ok_or_else(|| format!("{name} gave an answer this extension could not read."))
		}
		(Some(ResponseValue::Http { response }), _)
			if matches!(response.status, 400 | 401 | 403) =>
		{
			Err(format!("{name} refused the API key."))
		}
		(Some(ResponseValue::Http { response }), _) if response.status == 429 => {
			Err(format!("{name}'s limit was reached; try again later."))
		}
		(_, Some(error)) => Err(format!("Could not search {name}: {}", error.message)),
		_ => Err(format!("{name} did not answer.")),
	}
}

fn handle(input: ExtensionInvocation) -> ExtensionOutput {
	// The picker's dropdown sends its current choice; the saved setting is the fallback.
	let source = input
		.value("provider")
		.filter(|value| !value.is_empty())
		.or_else(|| input.setting_text("provider"))
		.unwrap_or("all")
		.to_owned();
	let rating = match input.setting_text("rating").unwrap_or("pg-13") {
		rating @ ("g" | "pg" | "pg-13" | "r") => rating.to_owned(),
		_ => "pg-13".to_owned(),
	};
	let wanted: &[&str] = match source.as_str() {
		"tenor" => &["tenor"],
		"giphy" => &["giphy"],
		"all" => &["tenor", "giphy"],
		// Klipy is Discord's own search, which Harmony runs itself.
		_ => &[],
	};
	let all = source == "all";
	// With every source on, a provider without a key is skipped instead of reported.
	let providers: Vec<&str> = wanted
		.iter()
		.copied()
		.filter(|provider| !all || input.secret_set(&format!("{provider}-key")))
		.collect();
	if let [provider] = providers.as_slice()
		&& !input.secret_set(&format!("{provider}-key"))
	{
		return problem(format!(
			"Add your {} API key in GIF Provider Switcher's settings.",
			name(provider)
		));
	}
	if providers.is_empty() {
		return ExtensionOutput::default();
	}
	let limit = MAX_GIF_RESULTS / providers.len();
	if input.response(providers[0]).is_none() {
		let query = input.value("query").unwrap_or_default();
		let mut output = ExtensionOutput::default();
		for provider in &providers {
			output.request(HostRequest::Http {
				id: (*provider).into(),
				request: Box::new(request(provider, &rating, query)),
			});
		}
		return output;
	}
	let mut gifs = Vec::new();
	let mut errors = Vec::new();
	for provider in &providers {
		match answer(&input, provider, limit) {
			Ok(found) => gifs.extend(found),
			Err(error) => errors.push(error),
		}
	}
	if gifs.is_empty() && !errors.is_empty() && !all {
		return problem(errors.join(" "));
	}
	ExtensionOutput {
		gifs,
		..Default::default()
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{SettingValue, testing::TestHost};

	const GIPHY: &str = r#"{"data":[{"id":"abc","title":"Cat","url":"https://giphy.com/gifs/cat-abc",
		"images":{"fixed_width":{"url":"https://media2.giphy.com/media/abc/200w.gif?cid=1&rid=200w.gif","width":"200","height":"150"}}},
		{"id":"broken","images":{}}]}"#;
	const TENOR: &str = r#"{"results":[{"id":"9","content_description":"Wave","itemurl":"https://tenor.com/view/wave-9",
		"media_formats":{"tinygif":{"url":"https://media.tenor.com/x/tenor.gif","dims":[220,124]}}}]}"#;

	fn search(source: &str, query: &str, keys: &[&str]) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "search".into();
		let values = &mut input.invocation.invocation.invocation.values;
		values.insert("query".into(), query.into());
		values.insert("provider".into(), source.into());
		input
			.settings
			.insert("provider".into(), SettingValue::Text("klipy".into()));
		for key in keys {
			input.secrets.insert(format!("{key}-key"), true);
		}
		input
	}

	#[test]
	fn giphy_results_become_picker_gifs_with_plain_previews() {
		let mut host = TestHost::default();
		host.web.insert(
			request("giphy", "pg-13", "cat dance").url,
			(200, GIPHY.into()),
		);
		let output = host
			.run(search("giphy", "cat dance", &["giphy"]), handle)
			.unwrap();
		let gifs = &output.gifs;
		assert_eq!(gifs.len(), 1);
		assert_eq!(gifs[0].id, "giphy-abc");
		assert_eq!(
			gifs[0].preview,
			"https://media2.giphy.com/media/abc/200w.gif"
		);
		assert_eq!((gifs[0].width, gifs[0].height), (200, 150));
		// The key is added by Harmony from the secret, never put in the address by this code.
		assert!(!host.requests[0].url.contains("api_key"));
		assert_eq!(host.requests[0].secret_params[0].secret, "giphy-key");
	}

	/// "All 3" asks every provider with a key in one round and skips the rest quietly; the
	/// dropdown's choice wins over the saved setting.
	#[test]
	fn all_sources_query_each_keyed_provider_and_skip_missing_keys() {
		let mut host = TestHost::default();
		host.web
			.insert(request("tenor", "pg-13", "").url, (200, TENOR.into()));
		host.web
			.insert(request("giphy", "pg-13", "").url, (200, GIPHY.into()));
		let output = host
			.run(search("all", "", &["tenor", "giphy"]), handle)
			.unwrap();
		let ids: Vec<_> = output.gifs.iter().map(|gif| gif.id.as_str()).collect();
		assert_eq!(ids, ["tenor-9", "giphy-abc"]);
		assert!(request("tenor", "g", "").url.contains("contentfilter=high"));

		let mut host = TestHost::default();
		host.web
			.insert(request("tenor", "pg-13", "hi").url, (200, TENOR.into()));
		let output = host.run(search("all", "hi", &["tenor"]), handle).unwrap();
		assert_eq!(host.requests.len(), 1);
		assert_eq!(output.gifs.len(), 1);

		// Klipy alone is Harmony's own search; a single provider without a key says so.
		let mut host = TestHost::default();
		assert!(
			host.run(search("klipy", "hi", &[]), handle)
				.unwrap()
				.gifs
				.is_empty()
		);
		let missing = host.run(search("tenor", "hi", &[]), handle).unwrap();
		assert!(host.requests.is_empty());
		assert!(!missing.output.output.panel.is_empty());
	}
}
