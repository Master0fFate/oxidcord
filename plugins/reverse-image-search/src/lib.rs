use plugin_sdk::model::UiContext;
use plugin_sdk::view;
use plugin_sdk::{HostError, Plugin, PluginExt, export_plugin, host, log, menu, mount, ui};

const PARENT_ITEM: &str = "search";
const TOAST_MS: u32 = 3000;
const MAX_QUERY_BYTES: usize = 1500;

const ENGINES: [(&str, &str, &str); 5] = [
	("lens", "Google Lens", "https://lens.google.com/uploadbyurl?url="),
	(
		"yandex",
		"Yandex",
		"https://yandex.com/images/search?rpt=imageview&url=",
	),
	("tineye", "TinEye", "https://tineye.com/search?url="),
	("saucenao", "SauceNAO", "https://saucenao.com/search.php?url="),
	(
		"bing",
		"Bing",
		"https://www.bing.com/images/search?view=detailv2&iss=sbi&q=imgurl:",
	),
];

#[derive(Default)]
struct ReverseImageSearch;

fn describe(error: &HostError) -> String {
	match error {
		HostError::Denied(_) => "A permission is missing".to_owned(),
		HostError::RateLimited => "One search every 10 seconds, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "Discord took too long".to_owned(),
		HostError::Unavailable => "Not available right now".to_owned(),
	}
}

fn notice(text: &str) {
	if let Err(error) = mount::toast([view::text(text)], TOAST_MS) {
		log::debug!("could not show the toast: {error:?}");
	}
}

fn encode(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	for byte in text.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
			out.push(char::from(byte));
		} else {
			out.push_str(&format!("%{byte:02X}"));
		}
	}
	out
}

impl Plugin for ReverseImageSearch {
	fn init(&mut self) -> Result<(), String> {
		let parent = menu::item(PARENT_ITEM, "Search image").icon("search");
		let engines = ENGINES
			.iter()
			.map(|(key, name, _)| menu::item(*key, *name).parent(PARENT_ITEM));
		if let Err(error) = menu::set(menu::IMAGE, std::iter::once(parent).chain(engines)) {
			log::warn!("could not add the menu entries: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for ReverseImageSearch {
	fn on_menu_item(&mut self, menu: &str, key: &str, _context: &UiContext) {
		if menu != menu::IMAGE {
			return;
		}
		let Some((_, _, search)) = ENGINES.iter().find(|(engine, _, _)| *engine == key) else {
			return;
		};
		let Some(image) = ui::context_detail().and_then(|detail| detail.image_url) else {
			return;
		};
		let image = encode(&image);
		if image.len() > MAX_QUERY_BYTES {
			return notice("That image link is too long");
		}
		if let Err(error) = host::open_url(&format!("{search}{image}")) {
			notice(&describe(&error));
		}
	}
}

export_plugin!(ReverseImageSearch, ext);
