use std::collections::{HashMap, VecDeque};

use plugin_sdk::model::{ClientEvent, DisplayOverrideFull, Presence, UserProfile};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{
	Event, HostError, Plugin, PluginExt, User, actions, data, export_plugin, host, log, storage, timer,
};

const FORMAT_KEY: &str = "pronounsFormat";
const SHOW_SELF_KEY: &str = "showSelf";
const LOAD_TAG: &str = "load";
const FIRST_LOAD_MS: u32 = 500;
const LOAD_INTERVAL_MS: u32 = 2000;
const MAX_TRACKED: usize = 4096;

struct Config {
	capitalized: bool,
	show_self: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			capitalized: false,
			show_self: true,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			FORMAT_KEY => self.capitalized = value.trim() == "capitalized",
			SHOW_SELF_KEY => self.show_self = !matches!(value.trim(), "false" | "0" | "off"),
			_ => {}
		}
	}

	fn format(&self, pronouns: &str) -> String {
		if self.capitalized {
			pronouns.to_owned()
		} else {
			pronouns.to_lowercase()
		}
	}
}

enum Entry {
	Queued,
	Loaded(Option<String>),
}

#[derive(Default)]
struct UserPronouns {
	config: Config,
	me: Option<u64>,
	entries: HashMap<u64, Entry>,
	queue: VecDeque<u64>,
	timer_armed: bool,
}

fn clean(raw: &str) -> Option<String> {
	let text: String = raw
		.trim()
		.chars()
		.filter(|ch| !matches!(ch, '\n' | '\r'))
		.collect();
	(!text.is_empty()).then_some(text)
}

fn profile_pronouns(profile: Option<UserProfile>) -> Option<String> {
	profile
		.and_then(|profile| profile.pronouns)
		.as_deref()
		.and_then(clean)
}

impl UserPronouns {
	fn pronouns_of(&mut self, user_id: u64) -> Option<String> {
		match self.entries.get(&user_id) {
			Some(Entry::Loaded(pronouns)) => return pronouns.clone(),
			Some(Entry::Queued) => return None,
			None => {}
		}
		match data::user_profile(user_id) {
			Ok(profile) => {
				let pronouns = profile_pronouns(profile);
				self.entries.insert(user_id, Entry::Loaded(pronouns.clone()));
				pronouns
			}
			Err(HostError::Unavailable) => {
				self.enqueue(user_id);
				None
			}
			Err(_) => None,
		}
	}

	fn enqueue(&mut self, user_id: u64) {
		if self.entries.len() >= MAX_TRACKED {
			self.entries.retain(|_, entry| matches!(entry, Entry::Queued));
		}
		self.entries.insert(user_id, Entry::Queued);
		self.queue.push_back(user_id);
		self.arm(FIRST_LOAD_MS);
	}

	fn arm(&mut self, delay_ms: u32) {
		if self.timer_armed {
			return;
		}
		match timer::once(delay_ms, LOAD_TAG) {
			Ok(_) => self.timer_armed = true,
			Err(error) => log::warn!("could not schedule a profile load: {error:?}"),
		}
	}

	fn load_next(&mut self) {
		let Some(user_id) = self.queue.pop_front() else {
			return;
		};
		match data::user_profile(user_id) {
			Ok(profile) => {
				let pronouns = profile_pronouns(profile);
				let found = pronouns.is_some();
				self.entries.insert(user_id, Entry::Loaded(pronouns));
				if found && let Err(error) = actions::invalidate_user(user_id) {
					log::debug!("could not refresh user {user_id}: {error:?}");
				}
			}
			Err(HostError::RateLimited) => self.queue.push_front(user_id),
			Err(error) => {
				log::debug!("could not load the profile of {user_id}: {error:?}");
				self.entries.insert(user_id, Entry::Loaded(None));
			}
		}
	}
}

impl Plugin for UserPronouns {
	fn init(&mut self) -> Result<(), String> {
		for key in [FORMAT_KEY, SHOW_SELF_KEY] {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		self.me = host::current_user().ok().map(|user| user.id);
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"pronounsFormat":{"type":"string","title":"Pronouns format","description":"The format for pronouns to appear in chat: lowercase, or capitalized to keep the casing from the profile","enum":["lowercase","capitalized"],"default":"lowercase"},"showSelf":{"type":"boolean","title":"Show yourself","description":"Show your own pronouns","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for UserPronouns {
	fn display_user_full(
		&mut self,
		_guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		if user.bot || (!self.config.show_self && self.me == Some(user.id)) {
			return None;
		}
		let pronouns = self.pronouns_of(user.id)?;
		Some(DisplayOverrideFull {
			name: None,
			suffix: None,
			color: None,
			badges: Vec::new(),
			avatar: None,
			banner: None,
			decoration: None,
			pronouns: Some(self.config.format(&pronouns)),
		})
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag != LOAD_TAG {
			return;
		}
		self.timer_armed = false;
		self.load_next();
		if !self.queue.is_empty() {
			self.arm(LOAD_INTERVAL_MS);
		}
	}

	fn on_client_event(&mut self, event: ClientEvent) {
		if matches!(event, ClientEvent::Ready) {
			self.entries.clear();
			self.queue.clear();
		}
	}
}

export_plugin!(UserPronouns, ext);
