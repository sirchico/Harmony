//! GIF Provider Switcher: choose where the GIF picker searches (Tenor, Giphy, Klipy, or all
//! three) from a dropdown beside its search field. Klipy is Harmony's built-in search; this
//! extension answers for Tenor and Giphy with their public web API keys, like the original
//! plugin, so no setup is needed. Written for Harmony using only the public API v2 SDK: a
//! `gif_search` action and `http` requests.
use harmony_extension_sdk::{
	Element, ExtensionInvocation, ExtensionOutput, GifCategoryResult, GifResult, HostRequest,
	HttpRequest, MAX_GIF_CATEGORIES, MAX_GIF_RESULTS, ResponseValue, serde_json,
};
use serde::Deserialize;

/// Tenor's public key from the Gboard app, as the original plugin uses.
const TENOR_KEY: &str = "3Z0688EVWYKH";
/// Giphy's public key from the giphy.com website, as the original plugin uses.
const GIPHY_KEY: &str = "Gc7131jiJuvI7IdN0HZ1D7nh0ow5BU6g";
/// GIFs asked for per search, shared between the providers searched.
const LIMIT: usize = MAX_GIF_RESULTS;

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

/// The request for one search of up to `limit` GIFs; an empty query asks for trending GIFs.
/// Both ask for only the renditions used, keeping answers small (about 2 KB per GIF instead
/// of 5 to 11 KB) so they fit an extension's input.
pub fn request(provider: &str, query: &str, limit: usize) -> HttpRequest {
	let query = query.trim();
	let search = |endpoint: &str| {
		if query.is_empty() {
			format!("{endpoint}?")
		} else {
			format!("search?q={}&", encode(query))
		}
	};
	if provider == "tenor" {
		HttpRequest::get(format!(
			"https://api.tenor.com/v1/{}key={TENOR_KEY}&limit={limit}&media_filter=minimal",
			search("trending")
		))
	} else {
		HttpRequest::get(format!(
			"https://api.giphy.com/v1/gifs/{}api_key={GIPHY_KEY}&limit={limit}&fields=id,title,url,images.fixed_height",
			search("trending")
		))
	}
}

/// The request for a provider's home categories. Giphy's list is large (about 12 KB per
/// category), so it asks for a dozen.
pub fn categories_request(provider: &str) -> HttpRequest {
	if provider == "tenor" {
		HttpRequest::get(format!(
			"https://api.tenor.com/v1/categories?key={TENOR_KEY}&type=featured"
		))
	} else {
		HttpRequest::get(format!(
			"https://api.giphy.com/v1/gifs/categories?api_key={GIPHY_KEY}&limit=12"
		))
	}
}

#[derive(Deserialize)]
struct TenorCategories {
	#[serde(default)]
	tags: Vec<TenorTag>,
}
#[derive(Deserialize)]
struct TenorTag {
	searchterm: String,
	#[serde(default)]
	image: Option<String>,
}
#[derive(Deserialize)]
struct GiphyCategories {
	#[serde(default)]
	data: Vec<GiphyCategory>,
}
#[derive(Deserialize)]
struct GiphyCategory {
	name: String,
	#[serde(default)]
	gif: Option<GiphyCategoryGif>,
}
#[derive(Deserialize)]
struct GiphyCategoryGif {
	images: GiphyImages,
}

/// Home categories from a provider's answer: a search term and a preview image each.
pub fn parse_categories(provider: &str, body: &str) -> Option<Vec<GifCategoryResult>> {
	let categories: Vec<GifCategoryResult> = if provider == "tenor" {
		let page: TenorCategories = serde_json::from_str(body).ok()?;
		page.tags
			.into_iter()
			.map(|tag| GifCategoryResult {
				name: short(tag.searchterm.trim(), 64),
				preview: tag.image.map(|image| plain(&image)),
			})
			.collect()
	} else {
		let page: GiphyCategories = serde_json::from_str(body).ok()?;
		page.data
			.into_iter()
			.map(|category| GifCategoryResult {
				name: short(category.name.trim(), 64),
				preview: category
					.gif
					.and_then(|gif| gif.images.fixed_height)
					.map(|image| plain(&image.url)),
			})
			.collect()
	};
	Some(
		categories
			.into_iter()
			.filter(|category| !category.name.is_empty())
			.take(MAX_GIF_CATEGORIES)
			.collect(),
	)
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
	title: String,
	#[serde(default)]
	h1_title: String,
	#[serde(default)]
	itemurl: String,
	#[serde(default)]
	media: Vec<TenorFormats>,
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
	fixed_height: Option<GiphyImage>,
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
				let formats = gif.media.into_iter().next()?;
				let media = formats.tinygif.or(formats.gif)?;
				let (width, height) = match media.dims.as_slice() {
					[width, height, ..] => (*width, *height),
					_ => (220, 220),
				};
				let url = if gif.itemurl.starts_with("https://") {
					gif.itemurl
				} else {
					media.url.clone()
				};
				let title = if gif.title.is_empty() {
					gif.h1_title
				} else {
					gif.title
				};
				Some(GifResult {
					id: format!("tenor-{}", short(&gif.id, 50)),
					title: short(&title, 200),
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
				let image = gif.images.fixed_height?;
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
		(Some(ResponseValue::Http { response }), _) if response.status == 429 => {
			Err(format!("{name}'s limit was reached; try again later."))
		}
		(Some(ResponseValue::Http { response }), _) => Err(format!(
			"{name} API request failed with status {}",
			response.status
		)),
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
		.unwrap_or("tenor")
		.to_owned();
	let providers: &[&str] = match source.as_str() {
		"tenor" => &["tenor"],
		"giphy" => &["giphy"],
		"all" => &["tenor", "giphy"],
		// Klipy is Discord's own search, which Harmony runs itself.
		_ => &[],
	};
	if providers.is_empty() {
		return ExtensionOutput::default();
	}
	let limit = LIMIT / providers.len();
	let query = input.value("query").unwrap_or_default().trim().to_owned();
	// The home page also shows categories, from the first provider like the original.
	let home = query.is_empty();
	let primary = providers[0];
	let categories_id = format!("{primary}-categories");
	// Giphy's category list nearly fills an input on its own: it is fetched in a round of
	// its own and carried to the next in the continuation.
	let separate = home && primary == "giphy";
	let searches = |output: &mut ExtensionOutput| {
		for provider in providers {
			output.request(HostRequest::Http {
				id: (*provider).into(),
				request: Box::new(request(provider, &query, limit)),
			});
		}
	};
	if input.round == 0 {
		let mut output = ExtensionOutput::default();
		if !separate {
			searches(&mut output);
		}
		if home {
			output.request(HostRequest::Http {
				id: categories_id.clone(),
				request: Box::new(categories_request(primary)),
			});
		}
		return output;
	}
	let read_categories = || {
		input
			.response(&categories_id)
			.and_then(|response| match &response.value {
				Some(ResponseValue::Http { response }) if response.status == 200 => {
					parse_categories(primary, &response.body)
				}
				_ => None,
			})
			.unwrap_or_default()
	};
	if separate && input.round == 1 {
		let mut output = ExtensionOutput {
			continuation: serde_json::to_string(&read_categories()).ok(),
			..Default::default()
		};
		searches(&mut output);
		return output;
	}
	let gif_categories = if separate {
		input
			.continuation
			.as_deref()
			.and_then(|text| serde_json::from_str(text).ok())
			.unwrap_or_default()
	} else if home {
		read_categories()
	} else {
		Vec::new()
	};
	let mut gifs = Vec::new();
	let mut errors = Vec::new();
	for provider in providers {
		match answer(&input, provider, limit) {
			Ok(found) => gifs.extend(found),
			Err(error) => errors.push(error),
		}
	}
	// With every source on, one failing provider still leaves the others' results.
	if gifs.is_empty() && !errors.is_empty() && source != "all" {
		return ExtensionOutput::panel(vec![Element::Text {
			text: errors.join(" "),
		}]);
	}
	ExtensionOutput {
		gifs,
		gif_categories,
		..Default::default()
	}
}

harmony_extension_sdk::export!(handle);

#[cfg(test)]
mod tests {
	use super::*;
	use harmony_extension_sdk::{SettingValue, testing::TestHost};

	const GIPHY: &str = r#"{"data":[{"id":"abc","title":"Cat","url":"https://giphy.com/gifs/cat-abc",
		"images":{"fixed_height":{"url":"https://media2.giphy.com/media/abc/200.gif?cid=1&rid=200.gif","width":"267","height":"200"}}},
		{"id":"broken","images":{}}]}"#;
	const TENOR: &str = r#"{"results":[{"id":"9","title":"","h1_title":"Wave","itemurl":"https://tenor.com/view/wave-9",
		"media":[{"tinygif":{"url":"https://media.tenor.com/x/tenor.gif","dims":[220,124]}}]}]}"#;

	fn search(source: &str, query: &str) -> ExtensionInvocation {
		let mut input = ExtensionInvocation::default();
		input.invocation.invocation.invocation.action = "search".into();
		let values = &mut input.invocation.invocation.invocation.values;
		values.insert("query".into(), query.into());
		values.insert("provider".into(), source.into());
		input
			.settings
			.insert("provider".into(), SettingValue::Text("klipy".into()));
		input
	}

	#[test]
	fn giphy_results_become_picker_gifs_with_plain_previews() {
		let mut host = TestHost::default();
		host.web
			.insert(request("giphy", "cat dance", 50).url, (200, GIPHY.into()));
		let output = host.run(search("giphy", "cat dance"), handle).unwrap();
		let gifs = &output.gifs;
		assert_eq!(gifs.len(), 1);
		assert_eq!(gifs[0].id, "giphy-abc");
		assert_eq!(
			gifs[0].preview,
			"https://media2.giphy.com/media/abc/200.gif"
		);
		assert_eq!((gifs[0].width, gifs[0].height), (267, 200));
		assert!(host.requests[0].url.contains("q=cat%20dance"));
	}

	/// "All 3" asks Tenor and Giphy in one round, a failing one leaves the other's results, and
	/// the dropdown's choice wins over the saved setting.
	#[test]
	fn all_sources_query_both_and_survive_one_failing() {
		let mut host = TestHost::default();
		host.web
			.insert(request("tenor", "", 25).url, (200, TENOR.into()));
		host.web
			.insert(request("giphy", "", 25).url, (200, GIPHY.into()));
		let output = host.run(search("all", ""), handle).unwrap();
		let ids: Vec<_> = output.gifs.iter().map(|gif| gif.id.as_str()).collect();
		assert_eq!(ids, ["tenor-9", "giphy-abc"]);
		assert_eq!(output.gifs[0].title, "Wave");
		assert!(request("tenor", "", 25).url.contains("/v1/trending?"));

		let mut host = TestHost::default();
		host.web
			.insert(request("tenor", "hi", 25).url, (200, TENOR.into()));
		host.web
			.insert(request("giphy", "hi", 25).url, (500, "{}".into()));
		let output = host.run(search("all", "hi"), handle).unwrap();
		assert_eq!(output.gifs.len(), 1);

		// Klipy alone is Harmony's own search.
		let mut host = TestHost::default();
		assert!(
			host.run(search("klipy", "hi"), handle)
				.unwrap()
				.gifs
				.is_empty()
		);
		assert!(host.requests.is_empty());
	}

	/// The home page carries the first provider's categories: Giphy's in a round of their own
	/// (carried in the continuation), Tenor's alongside its trending GIFs.
	#[test]
	fn home_pages_carry_the_first_providers_categories() {
		let giphy_categories = r#"{"data":[{"name":"Actions","gif":{"images":{"fixed_height":{"url":"https://media1.giphy.com/media/a/200.gif?cid=1","width":"200","height":"200"}}}},{"name":" "}]}"#;
		let tenor_categories =
			r#"{"tags":[{"searchterm":"kiss","image":"https://media.tenor.com/k/kiss.gif"}]}"#;
		let mut host = TestHost::default();
		host.web.insert(
			categories_request("giphy").url,
			(200, giphy_categories.into()),
		);
		host.web
			.insert(request("giphy", "", 50).url, (200, GIPHY.into()));
		let output = host.run(search("giphy", ""), handle).unwrap();
		assert_eq!(output.gifs.len(), 1);
		assert_eq!(output.gif_categories.len(), 1);
		assert_eq!(output.gif_categories[0].name, "Actions");
		assert_eq!(
			output.gif_categories[0].preview.as_deref(),
			Some("https://media1.giphy.com/media/a/200.gif")
		);
		// Categories first, then trending: two rounds.
		assert_eq!(host.requests[0].url, categories_request("giphy").url);

		let mut host = TestHost::default();
		host.web.insert(
			categories_request("tenor").url,
			(200, tenor_categories.into()),
		);
		host.web
			.insert(request("tenor", "", 25).url, (200, TENOR.into()));
		host.web
			.insert(request("giphy", "", 25).url, (200, GIPHY.into()));
		let output = host.run(search("all", ""), handle).unwrap();
		assert_eq!(output.gifs.len(), 2);
		assert_eq!(output.gif_categories[0].name, "kiss");
		assert_eq!(host.requests.len(), 3);
		// A search shows no categories and fetches none.
		let mut host = TestHost::default();
		host.web
			.insert(request("tenor", "cat", 50).url, (200, TENOR.into()));
		let output = host.run(search("tenor", "cat"), handle).unwrap();
		assert!(output.gif_categories.is_empty());
		assert_eq!(host.requests.len(), 1);
	}
}
