//! Installed-family enumeration and one bounded face load; persistence uses the cache worker.
use std::sync::mpsc;
use ui::fonts::CustomFont;

pub type Selected = Result<Option<CustomFont>, &'static str>;
/// Upper bound on listed families; real systems carry a few hundred to a couple thousand.
const MAX_FAMILIES: usize = 4096;

fn collection() -> fontique::Collection {
	fontique::Collection::new(fontique::CollectionOptions {
		shared: false,
		system_fonts: true,
	})
}

/// Sorted, user-visible installed family names, enumerated off the UI thread.
pub fn installed(
	runtime: &tokio::runtime::Runtime,
	ctx: &eframe::egui::Context,
) -> mpsc::Receiver<Vec<String>> {
	let (send, receive) = mpsc::sync_channel(1);
	let ctx = ctx.clone();
	runtime.spawn(async move {
		let families = tokio::task::spawn_blocking(|| {
			let mut names: Vec<String> = collection()
				.family_names()
				// Dot-prefixed families are private system UI faces.
				.filter(|name| {
					!name.is_empty()
						&& !name.starts_with('.')
						&& name.len() <= 128
						&& !name.chars().any(char::is_control)
				})
				.take(MAX_FAMILIES)
				.map(str::to_owned)
				.collect();
			names.sort_by_cached_key(|name| name.to_lowercase());
			names.dedup();
			names
		})
		.await
		.unwrap_or_default();
		let _ = send.send(families);
		ctx.request_repaint();
	});
	receive
}

/// Load the family's upright regular face as a standalone, validated font.
pub fn load(
	runtime: &tokio::runtime::Runtime,
	ctx: &eframe::egui::Context,
	family: String,
) -> mpsc::Receiver<Selected> {
	let (send, receive) = mpsc::sync_channel(1);
	let ctx = ctx.clone();
	runtime.spawn(async move {
		let result = tokio::task::spawn_blocking(move || read_family(family).map(Some))
			.await
			.unwrap_or(Err("Font loading interrupted. Try again."));
		let _ = send.send(result);
		ctx.request_repaint();
	});
	receive
}

fn read_family(name: String) -> Result<CustomFont, &'static str> {
	let mut collection = collection();
	let family = collection
		.family_by_name(&name)
		.ok_or("This font is no longer installed.")?;
	// Every upright face at normal width, so Medium and SemiBold text use their real faces.
	let mut fonts: Vec<_> = family
		.fonts()
		.iter()
		.filter(|font| font.style() == fontique::FontStyle::Normal)
		.filter(|font| (font.width().ratio() - 1.0).abs() < 0.01)
		.take(ui::fonts::MAX_CUSTOM_FONT_FACES)
		.collect();
	if fonts.is_empty() {
		fonts.extend(family.default_font());
	}
	if fonts.is_empty() {
		return Err("This font has no usable style.");
	}
	let mut faces = Vec::with_capacity(fonts.len());
	for font in fonts {
		let blob = font.load(None).ok_or("Could not read the font.")?;
		faces.push(ui::fonts::standalone_face(blob.as_ref(), font.index())?);
	}
	CustomFont::from_faces(name, faces)
}

/// Pick a font file or a ZIP of one family's weights, then read it off the UI thread.
pub fn import(
	runtime: &tokio::runtime::Runtime,
	ctx: &eframe::egui::Context,
	window: std::sync::Arc<winit::window::Window>,
) -> mpsc::Receiver<Selected> {
	let (send, receive) = mpsc::sync_channel(1);
	let ctx = ctx.clone();
	let picked = platform::save::font_source(window);
	runtime.spawn(async move {
		let result = match picked.await {
			Some(path) => tokio::task::spawn_blocking(move || read_file(&path).map(Some))
				.await
				.unwrap_or(Err("Font import interrupted. Try again.")),
			None => Ok(None),
		};
		let _ = send.send(result);
		ctx.request_repaint();
	});
	receive
}

/// Largest ZIP accepted; a family of every weight and italic is a few megabytes.
const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
/// Most entries read from one archive, fonts or not.
const MAX_ARCHIVE_ENTRIES: usize = 1024;
/// Decompressed bytes read from one archive across all font entries.
const MAX_EXTRACTED_BYTES: u64 = 128 * 1024 * 1024;

fn read_file(path: &std::path::Path) -> Result<CustomFont, &'static str> {
	use std::io::Read;
	let stem = path
		.file_stem()
		.map(|stem| stem.to_string_lossy().into_owned())
		.unwrap_or_default();
	let mut file = std::fs::File::open(path).map_err(|_| "Could not read the font.")?;
	let size = file
		.metadata()
		.map_err(|_| "Could not read the font.")?
		.len();
	let mut magic = [0_u8; 4];
	let zipped = file.read_exact(&mut magic).is_ok() && magic == *b"PK\x03\x04";
	if zipped {
		if size > MAX_ARCHIVE_BYTES {
			return Err("Choose a ZIP up to 128 MiB.");
		}
		let faces =
			read_archive(std::fs::File::open(path).map_err(|_| "Could not read the ZIP.")?)?;
		return CustomFont::from_faces(stem, faces);
	}
	if size > ui::fonts::MAX_CUSTOM_FONT_BYTES as u64 {
		return Err("Choose a font up to 32 MiB.");
	}
	let bytes = std::fs::read(path).map_err(|_| "Could not read the font.")?;
	CustomFont::from_faces(stem, vec![bytes])
}

/// The TTF and OTF members of a ZIP, bounded by count and by decompressed size.
fn read_archive<R: std::io::Read + std::io::Seek>(reader: R) -> Result<Vec<Vec<u8>>, &'static str> {
	use std::io::Read;
	let mut archive = zip::ZipArchive::new(reader).map_err(|_| "This is not a readable ZIP.")?;
	if archive.len() > MAX_ARCHIVE_ENTRIES {
		return Err("This ZIP has too many files.");
	}
	let mut faces = Vec::new();
	let mut extracted = 0_u64;
	for index in 0..archive.len() {
		let mut entry = archive
			.by_index(index)
			.map_err(|_| "Could not read a file in the ZIP.")?;
		let name = entry
			.name()
			.map(|name| name.to_ascii_lowercase())
			.unwrap_or_default();
		let file_name = name.rsplit('/').next().unwrap_or_default();
		let font = name.ends_with(".ttf") || name.ends_with(".otf");
		// macOS archives carry `__MACOSX/._name` resource forks next to the real files.
		if entry.is_dir()
			|| entry.encrypted()
			|| !font || name.contains("__macosx/")
			|| file_name.starts_with("._")
		{
			continue;
		}
		if faces.len() == ui::fonts::MAX_CUSTOM_FONT_FACES {
			return Err("This ZIP has too many fonts.");
		}
		// The header size can lie; never read more than one font's limit from an entry.
		let limit = ui::fonts::MAX_CUSTOM_FONT_BYTES as u64;
		let mut bytes = Vec::with_capacity(entry.size().min(limit) as usize);
		(&mut entry)
			.take(limit + 1)
			.read_to_end(&mut bytes)
			.map_err(|_| "Could not read a font in the ZIP.")?;
		if bytes.len() as u64 > limit {
			return Err("A font in the ZIP is larger than 32 MiB.");
		}
		extracted += bytes.len() as u64;
		if extracted > MAX_EXTRACTED_BYTES {
			return Err("The fonts in this ZIP are too large.");
		}
		faces.push(bytes);
	}
	if faces.is_empty() {
		return Err("No TTF or OTF fonts were found in the ZIP.");
	}
	Ok(faces)
}

#[cfg(all(debug_assertions, feature = "demo"))]
fn read(path: &std::path::Path) -> Result<CustomFont, &'static str> {
	let bytes = std::fs::read(path).map_err(|_| "Could not read the font.")?;
	CustomFont::new(
		path.file_stem()
			.unwrap_or_default()
			.to_string_lossy()
			.into_owned(),
		bytes,
	)
}

#[cfg(all(debug_assertions, feature = "demo"))]
pub fn debug_check() {
	use eframe::egui::{self, FontFamily};
	use std::path::Path;
	use ui::fonts::MAX_CUSTOM_FONT_BYTES;
	let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
	let font = read(&assets.join("NotoSans-Regular.ttf")).unwrap();
	let replacement = read(&assets.join("NotoSans-SemiBold.ttf")).unwrap();
	assert!(CustomFont::new("Invalid".into(), b"not a font".to_vec()).is_err());
	let mut random = [0_u8; 8];
	getrandom::fill(&mut random).unwrap();
	let directory = std::env::temp_dir().join(format!("harmony-font-debug-{random:02x?}"));
	std::fs::create_dir(&directory).unwrap();
	let oversized = directory.join("large.ttf");
	std::fs::File::create(&oversized)
		.unwrap()
		.set_len(MAX_CUSTOM_FONT_BYTES as u64 + 1)
		.unwrap();
	assert!(read(&oversized).is_err());
	let path = directory.join("preferences.sqlite3");
	let store = local_store::LocalStore::open(&path).unwrap();
	store
		.save_custom_font(Some((&font.name, &font.bytes())))
		.unwrap();
	let preferences = local_store::AppPreferences {
		hide_window_decorations: true,
		..Default::default()
	};
	store.save_app_preferences(&preferences).unwrap();
	drop(store);
	let store = local_store::LocalStore::open(&path).unwrap();
	let (name, bytes) = store.custom_font().unwrap().unwrap();
	let restored = CustomFont::new(name, bytes).unwrap();
	assert_eq!(restored.bytes(), font.bytes());
	assert!(store.save_custom_font(Some(("", &font.bytes()))).is_err());
	assert_eq!(store.custom_font().unwrap().unwrap().0, font.name);
	let mut view = ui::MessagingUi::default();
	crate::app_settings::Settings {
		current: store.app_preferences().unwrap(),
		..Default::default()
	}
	.apply(&mut view);
	assert!(view.hide_window_decorations);
	store.save_custom_font(None).unwrap();
	assert!(store.custom_font().unwrap().is_none());
	drop(store);

	let ctx = egui::Context::default();
	ui::fonts::install(&ctx);
	ui::fonts::apply_custom(&ctx, Some(&restored));
	let frame = |text: &str| {
		ctx.run_ui(egui::RawInput::default(), |ui| {
			ui.label(text);
		})
		.drop_without_applying_deltas();
	};
	frame("Custom font 日本語");
	let original = ui::fonts::revision(&ctx);
	ui::fonts::apply_custom(&ctx, Some(&replacement));
	for _ in 0..100 {
		frame("Replacement font 日本語");
		if ctx.fonts(|fonts| fonts.definitions().font_data.contains_key("Noto Sans CJK")) {
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(10));
	}
	assert_ne!(original, ui::fonts::revision(&ctx));
	ctx.fonts(|fonts| {
		let definitions = fonts.definitions();
		assert!(definitions.font_data.contains_key("Noto Sans CJK"));
		assert_eq!(
			definitions.families[&FontFamily::Proportional][0],
			"Harmony Custom"
		);
		assert_eq!(
			definitions.font_data["Harmony Custom"].bytes(),
			replacement.bytes()
		);
		assert!(
			!definitions.families[&FontFamily::Monospace]
				.iter()
				.any(|name| name.starts_with("Harmony Custom"))
		);
	});
	ui::fonts::apply_custom(&ctx, None);
	frame("Back to Noto Sans");
	ctx.fonts(|fonts| {
		assert_eq!(
			fonts.definitions().families[&FontFamily::Proportional][0],
			"Noto Sans"
		);
		assert!(fonts.definitions().font_data.contains_key("Noto Sans CJK"));
		assert!(!fonts.definitions().font_data.contains_key("Harmony Custom"));
	});
	let collection = ctx.fonts(|fonts| {
		fonts.definitions().font_data["Noto Sans CJK"]
			.bytes()
			.to_vec()
	});
	for index in [0, 2, 3] {
		let bytes = ui::fonts::standalone_face(&collection, index).unwrap();
		assert!(bytes.len() > 8 * 1024 * 1024);
		let font = CustomFont::new("CJK".into(), bytes).unwrap();
		let store = local_store::LocalStore::open(&path).unwrap();
		store
			.save_custom_font(Some((&font.name, &font.bytes())))
			.unwrap();
		drop(store);
		let store = local_store::LocalStore::open(&path).unwrap();
		assert_eq!(store.custom_font().unwrap().unwrap().1, font.bytes());
	}
	ui::fonts::apply_custom(&ctx, Some(&replacement));
	ui::i18n::set_current(ui::i18n::Language::English);
	frame("中文 日本語");
	frame("中文 日本語");
	for (language, index) in [
		(ui::i18n::Language::ChineseSimplified, 2),
		(ui::i18n::Language::ChineseTraditional, 3),
		(ui::i18n::Language::Japanese, 0),
	] {
		let before = ui::fonts::revision(&ctx);
		ui::i18n::set_current(language);
		frame("中文 日本語");
		frame("中文 日本語");
		assert_ne!(before, ui::fonts::revision(&ctx));
		ctx.fonts(|fonts| {
			let definitions = fonts.definitions();
			assert_eq!(definitions.font_data["Noto Sans CJK"].index, index);
			assert_eq!(
				definitions.families[&FontFamily::Proportional][0],
				"Harmony Custom"
			);
		});
	}
	ui::i18n::set_current(ui::i18n::Language::System);
	std::fs::remove_dir_all(directory).unwrap();
	println!(
		"Font debug check passed: bounded CJK import and persistence, locale switching, invalid input, replacement during CJK loading, reset, and saved decoration preference."
	);
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write;

	fn asset(name: &str) -> Vec<u8> {
		std::fs::read(
			std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
				.join("../../assets/fonts")
				.join(name),
		)
		.unwrap()
	}

	#[test]
	fn family_zip_picks_faces_by_declared_weight_and_survives_saving() {
		let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
		let options = zip::write::SimpleFileOptions::default();
		// Names deliberately disagree with the weights inside; macOS junk must be skipped.
		for (name, bytes) in [
			("Family/A.ttf", asset("NotoSans-SemiBold.ttf")),
			("Family/B.TTF", asset("NotoSans-Regular.ttf")),
			("Family/C.otf", asset("NotoSans-Medium.ttf")),
			("__MACOSX/Family/._B.TTF", b"resource fork".to_vec()),
			("Family/._C.otf", b"resource fork".to_vec()),
			("Family/OFL.txt", b"licence".to_vec()),
		] {
			writer.start_file(name, options).unwrap();
			writer.write_all(&bytes).unwrap();
		}
		let archive = writer.finish().unwrap().into_inner();
		let faces = read_archive(std::io::Cursor::new(archive)).unwrap();
		assert_eq!(faces.len(), 3);
		let font = CustomFont::from_faces("Fallback".into(), faces).unwrap();
		assert_eq!(font.name, "Noto Sans");
		assert_eq!(font.weights(), [400, 500, 600]);
		let restored = CustomFont::new(font.name.clone(), font.bytes()).unwrap();
		assert_eq!(restored.weights(), [400, 500, 600]);
		assert_eq!(restored.bytes(), font.bytes());
		// A lone face is still stored as the plain font file older builds read.
		let single = CustomFont::from_faces("Single".into(), vec![asset("NotoSans-Regular.ttf")]);
		assert_eq!(single.unwrap().bytes(), asset("NotoSans-Regular.ttf"));
		// Truncated bundles are rejected rather than half-restored.
		let mut damaged = font.bytes();
		damaged.truncate(damaged.len() - 10);
		assert!(CustomFont::new("Damaged".into(), damaged).is_err());
	}

	#[test]
	fn archives_without_fonts_or_with_oversized_entries_are_rejected() {
		let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
		writer
			.start_file("readme.txt", zip::write::SimpleFileOptions::default())
			.unwrap();
		writer.write_all(b"no fonts").unwrap();
		let archive = writer.finish().unwrap().into_inner();
		assert!(read_archive(std::io::Cursor::new(archive)).is_err());

		let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
		writer
			.start_file("huge.ttf", zip::write::SimpleFileOptions::default())
			.unwrap();
		writer
			.write_all(&vec![0_u8; ui::fonts::MAX_CUSTOM_FONT_BYTES + 1])
			.unwrap();
		let archive = writer.finish().unwrap().into_inner();
		assert!(read_archive(std::io::Cursor::new(archive)).is_err());
	}
}
