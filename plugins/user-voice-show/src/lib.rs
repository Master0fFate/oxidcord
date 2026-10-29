use std::collections::HashSet;

use plugin_sdk::model::{Badge, DisplayOverrideFull, Presence};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, User, actions, data, export_plugin, log, storage, timer};

const REFRESH_MS: u32 = 10_000;
const REFRESH_TAG: &str = "refresh";
const SHOW_KEY: &str = "show";

struct UserVoiceShow {
	show: bool,
	channel_name: String,
	participants: HashSet<u64>,
}

impl Default for UserVoiceShow {
	fn default() -> Self {
		Self {
			show: true,
			channel_name: String::new(),
			participants: HashSet::new(),
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl UserVoiceShow {
	fn sync(&mut self) {
		let (channel_name, participants) = match data::voice_detail() {
			Ok(Some(call)) => (call.channel_name, call.base.participants.into_iter().collect()),
			Ok(None) => (String::new(), HashSet::new()),
			Err(error) => {
				log::warn!("could not read the voice call: {error:?}");
				return;
			}
		};
		let renamed = channel_name != self.channel_name;
		let changed: Vec<u64> = if renamed {
			self.participants.union(&participants).copied().collect()
		} else {
			self.participants
				.symmetric_difference(&participants)
				.copied()
				.collect()
		};
		self.channel_name = channel_name;
		self.participants = participants;
		for user_id in changed {
			if let Err(error) = actions::invalidate_user(user_id) {
				log::debug!("could not refresh user {user_id}: {error:?}");
			}
		}
	}
}

impl Plugin for UserVoiceShow {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{SHOW_KEY}")) {
			self.show = parse_flag(&String::from_utf8_lossy(&saved));
		}
		self.sync();
		if let Err(error) = timer::every(REFRESH_MS, REFRESH_TAG) {
			log::warn!("could not start the refresh timer: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::VoiceChanged(_) => self.sync(),
			Event::SettingChanged(SettingChange { key, value }) if key == SHOW_KEY => {
				self.show = parse_flag(&value);
			}
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"show":{"type":"boolean","title":"Show indicator","description":"Show a speaker badge next to every user who is in the voice call you are in","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for UserVoiceShow {
	fn display_user_full(
		&mut self,
		_guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		if !self.show || !self.participants.contains(&user.id) {
			return None;
		}
		Some(DisplayOverrideFull {
			name: None,
			suffix: None,
			color: None,
			badges: vec![Badge {
				icon: Some("volume-2".to_owned()),
				image: None,
				label: None,
				tooltip: format!("In voice: {}", self.channel_name),
				color: None,
				key: None,
			}],
			avatar: None,
			banner: None,
			decoration: None,
			pronouns: None,
		})
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == REFRESH_TAG {
			self.sync();
		}
	}
}

export_plugin!(UserVoiceShow, ext);
