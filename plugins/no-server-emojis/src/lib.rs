use plugin_sdk::model::EmojiInfo;
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, export_plugin, host, storage};

const SHOWN_KEY: &str = "shownEmojis";

#[derive(Clone, Copy, PartialEq)]
enum Shown {
	OnlyUnicode,
	CurrentServer,
	All,
}

impl Shown {
	fn parse(raw: &str) -> Self {
		match raw.trim() {
			"currentServer" => Self::CurrentServer,
			"all" => Self::All,
			_ => Self::OnlyUnicode,
		}
	}
}

struct NoServerEmojis {
	shown: Shown,
}

impl Default for NoServerEmojis {
	fn default() -> Self {
		Self {
			shown: Shown::OnlyUnicode,
		}
	}
}

impl Plugin for NoServerEmojis {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{SHOWN_KEY}")) {
			self.shown = Shown::parse(&String::from_utf8_lossy(&saved));
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == SHOWN_KEY
		{
			self.shown = Shown::parse(&value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"shownEmojis":{"type":"string","title":"Shown emojis","description":"onlyUnicode shows only unicode emojis, currentServer adds the emojis of the current server, all shows every emoji","enum":["onlyUnicode","currentServer","all"],"default":"onlyUnicode"}}}"#
			.to_owned()
	}
}

impl PluginExt for NoServerEmojis {
	fn autocomplete_emoji(&mut self, _query: &str, candidates: &[EmojiInfo]) -> Vec<u32> {
		let guild_id = match self.shown {
			Shown::All => return Vec::new(),
			Shown::OnlyUnicode => None,
			Shown::CurrentServer => host::current_channel()
				.ok()
				.flatten()
				.and_then(|(guild_id, _)| guild_id),
		};
		let kept: Vec<u32> = candidates
			.iter()
			.enumerate()
			.filter(|(_, info)| info.emoji.id.is_none() || (guild_id.is_some() && info.guild_id == guild_id))
			.map(|(index, _)| index as u32)
			.collect();
		if kept.len() == candidates.len() {
			Vec::new()
		} else {
			kept
		}
	}
}

export_plugin!(NoServerEmojis, ext);
