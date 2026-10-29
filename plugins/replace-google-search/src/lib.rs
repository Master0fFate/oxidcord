use plugin_sdk::model::UiContext;
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{Event, HostError, Plugin, PluginExt, export_plugin, host, log, menu, mount, storage, ui};

const ENGINE_KEY: &str = "replacementEngine";
const NAME_KEY: &str = "customEngineName";
const URL_KEY: &str = "customEngineURL";
const OFF: &str = "off";
const CUSTOM: &str = "custom";
const SINGLE_ITEM: &str = "search";
const PARENT_ITEM: &str = "search-text";
const ENGINE_PREFIX: &str = "engine-";
const MAX_SELECTION_CHARS: usize = 500;
const MAX_QUERY_BYTES: usize = 1500;
const MAX_NAME_CHARS: usize = 40;
const TOAST_MS: u32 = 3000;

const ENGINES: [(&str, &str); 12] = [
	("Google", "https://www.google.com/search?q="),
	("DuckDuckGo", "https://duckduckgo.com/?q="),
	("Brave", "https://search.brave.com/search?q="),
	("Bing", "https://www.bing.com/search?q="),
	("Yahoo", "https://search.yahoo.com/search?p="),
	("Yandex", "https://yandex.com/search/?text="),
	("Ecosia", "https://www.ecosia.org/search?q="),
	("GitHub", "https://github.com/search?q="),
	("Reddit", "https://www.reddit.com/search?q="),
	("Wikipedia", "https://wikipedia.org/w/index.php?search="),
	("Startpage", "https://www.startpage.com/sp/search?query="),
	("Kagi", "https://kagi.com/search?q="),
];

struct Config {
	engine: String,
	custom_name: String,
	custom_url: String,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			engine: OFF.to_owned(),
			custom_name: String::new(),
			custom_url: String::new(),
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		match key {
			ENGINE_KEY => self.engine = value.to_owned(),
			NAME_KEY => self.custom_name = value.chars().take(MAX_NAME_CHARS).collect(),
			URL_KEY => self.custom_url = value.to_owned(),
			_ => {}
		}
	}

	fn custom(&self) -> Option<(&str, &str)> {
		let usable = !self.custom_name.is_empty()
			&& (self.custom_url.starts_with("https://") || self.custom_url.starts_with("http://"));
		usable.then_some((self.custom_name.as_str(), self.custom_url.as_str()))
	}

	fn engines(&self) -> Vec<(&str, &str)> {
		let mut engines = ENGINES.to_vec();
		engines.extend(self.custom());
		engines
	}

	fn replacement(&self) -> Option<(&str, &str)> {
		if self.engine == CUSTOM {
			return self.custom();
		}
		ENGINES.iter().copied().find(|(name, _)| *name == self.engine)
	}
}

#[derive(Default)]
struct ReplaceGoogleSearch {
	config: Config,
}

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

impl ReplaceGoogleSearch {
	fn sync_menu(&self) {
		let items = match self.config.replacement() {
			Some((name, _)) => vec![menu::item(SINGLE_ITEM, format!("Search with {name}")).icon("search")],
			None => {
				let parent = menu::item(PARENT_ITEM, "Search text").icon("search");
				let engines = self
					.config
					.engines()
					.into_iter()
					.enumerate()
					.map(|(index, (name, _))| {
						menu::item(format!("{ENGINE_PREFIX}{index}"), name).parent(PARENT_ITEM)
					});
				std::iter::once(parent).chain(engines).collect()
			}
		};
		if let Err(error) = menu::set(menu::SELECTION, items) {
			log::warn!("could not update the menu entries: {error:?}");
		}
	}

	fn target(&self, key: &str) -> Option<&str> {
		if key == SINGLE_ITEM {
			return self.config.replacement().map(|(_, url)| url);
		}
		let index: usize = key.strip_prefix(ENGINE_PREFIX)?.parse().ok()?;
		self.config.engines().get(index).map(|(_, url)| *url)
	}
}

impl Plugin for ReplaceGoogleSearch {
	fn init(&mut self) -> Result<(), String> {
		for key in [ENGINE_KEY, NAME_KEY, URL_KEY] {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		self.sync_menu();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
			self.sync_menu();
		}
	}

	fn settings_schema(&self) -> String {
		let engines: Vec<String> = [OFF, CUSTOM]
			.into_iter()
			.chain(ENGINES.iter().map(|(name, _)| *name))
			.map(|name| format!("\"{name}\""))
			.collect();
		format!(
			r#"{{"type":"object","properties":{{"customEngineName":{{"type":"string","title":"Custom engine name","description":"Name of the custom search engine"}},"customEngineURL":{{"type":"string","title":"Custom engine URL","description":"The URL of your engine, for example https://google.com/search?q="}},"replacementEngine":{{"type":"string","title":"Replacement engine","description":"Replace the submenu with one specific search engine","enum":[{}],"default":"off"}}}}}}"#,
			engines.join(",")
		)
	}
}

impl PluginExt for ReplaceGoogleSearch {
	fn on_menu_item(&mut self, menu: &str, key: &str, _context: &UiContext) {
		if menu != menu::SELECTION {
			return;
		}
		let Some(base) = self.target(key) else {
			return;
		};
		let Some(selection) = ui::context_detail().and_then(|detail| detail.selection) else {
			return;
		};
		let query: String = selection.trim().chars().take(MAX_SELECTION_CHARS).collect();
		let query = encode(&query);
		if query.is_empty() || query.len() > MAX_QUERY_BYTES {
			return notice("That selection cannot be searched");
		}
		if let Err(error) = host::open_url(&format!("{base}{query}")) {
			notice(&describe(&error));
		}
	}
}

export_plugin!(ReplaceGoogleSearch, ext);
