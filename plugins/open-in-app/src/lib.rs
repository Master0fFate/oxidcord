use plugin_sdk::model::{LinkVerdict, UiContext};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, export_plugin, storage};

const SETTING_KEYS: [&str; 4] = ["spotify", "steam", "tidal", "itunes"];

type Rule = fn(&str, &str, &[&str]) -> Option<String>;

const RULES: [Rule; 4] = [spotify, steam, tidal, apple_music];

struct OpenInApp {
	enabled: [bool; 4],
}

impl Default for OpenInApp {
	fn default() -> Self {
		Self { enabled: [true; 4] }
	}
}

fn split(url: &str) -> Option<(&str, Vec<&str>)> {
	let rest = url.strip_prefix("https://")?;
	let rest = rest.split(['?', '#']).next()?;
	let (host, path) = rest.split_once('/')?;
	Some((host, path.split('/').filter(|part| !part.is_empty()).collect()))
}

fn spotify(_url: &str, host: &str, path: &[&str]) -> Option<String> {
	if host != "open.spotify.com" {
		return None;
	}
	let path = match path {
		[locale, rest @ ..] if locale.starts_with("intl-") => rest,
		_ => path,
	};
	match path {
		[
			kind @ ("track" | "album" | "artist" | "playlist" | "user" | "episode" | "prerelease"),
			id,
			..,
		] => Some(format!("spotify:{kind}:{id}")),
		_ => None,
	}
}

fn steam(url: &str, host: &str, path: &[&str]) -> Option<String> {
	let known = matches!(
		host,
		"store.steampowered.com" | "help.steampowered.com" | "steamcommunity.com"
	);
	(known && !path.is_empty()).then(|| format!("steam://openurl/{url}"))
}

fn tidal(_url: &str, host: &str, path: &[&str]) -> Option<String> {
	if !matches!(host, "tidal.com" | "listen.tidal.com") {
		return None;
	}
	let path = match path {
		["browse", rest @ ..] => rest,
		_ => path,
	};
	match path {
		[
			kind @ ("track" | "album" | "artist" | "playlist" | "user" | "video" | "mix"),
			id,
			..,
		] if !id.is_empty()
			&& id
				.bytes()
				.all(|byte| matches!(byte, b'a'..=b'f' | b'0'..=b'9' | b'-')) =>
		{
			Some(format!("tidal://{kind}/{id}"))
		}
		_ => None,
	}
}

fn apple_music(_url: &str, host: &str, path: &[&str]) -> Option<String> {
	if !matches!(host, "music.apple.com" | "geo.music.apple.com") {
		return None;
	}
	let path = match path {
		[locale, rest @ ..] if locale.len() == 2 => rest,
		_ => path,
	};
	match path {
		[
			kind @ ("album" | "artist" | "playlist" | "song" | "curator"),
			name,
		] => Some(format!("music://music.apple.com/us/{kind}/{name}")),
		[
			kind @ ("album" | "artist" | "playlist" | "song" | "curator"),
			name,
			id,
			..,
		] => Some(format!("music://music.apple.com/us/{kind}/{name}/{id}")),
		_ => None,
	}
}

impl OpenInApp {
	fn set(&mut self, key: &str, value: &str) {
		if let Some(index) = SETTING_KEYS.iter().position(|known| *known == key) {
			self.enabled[index] = !matches!(value.trim(), "false" | "0" | "off");
		}
	}

	fn rewrite(&self, url: &str) -> Option<String> {
		let (host, path) = split(url)?;
		RULES
			.iter()
			.zip(self.enabled)
			.filter(|(_, enabled)| *enabled)
			.find_map(|(rule, _)| rule(url, host, &path))
	}
}

impl Plugin for OpenInApp {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"spotify":{"type":"boolean","title":"Spotify","description":"Open Spotify links in the Spotify app","default":true},"steam":{"type":"boolean","title":"Steam","description":"Open Steam links in the Steam app","default":true},"tidal":{"type":"boolean","title":"Tidal","description":"Open Tidal links in the Tidal app","default":true},"itunes":{"type":"boolean","title":"iTunes","description":"Open Apple Music links in the Apple Music app","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for OpenInApp {
	fn on_link_click(&mut self, url: &str, _context: &UiContext) -> LinkVerdict {
		match self.rewrite(url) {
			Some(app) => LinkVerdict::Open(app),
			None => LinkVerdict::Pass,
		}
	}
}

export_plugin!(OpenInApp, ext);
