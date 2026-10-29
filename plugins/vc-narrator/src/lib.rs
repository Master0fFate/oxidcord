use plugin_sdk::types::SettingChange;
use plugin_sdk::{
	Event, Plugin, PluginExt, VoiceInfo, actions, data, export_plugin, host, log, storage, timer,
};

const WATCH_TAG: &str = "watch";
const WATCH_MS: u32 = 1_000;
const MAX_SPOKEN_CHARS: usize = 300;
const MIN_RATE: f32 = 0.5;
const MAX_RATE: f32 = 2.0;
const KINDS: usize = 7;

#[derive(Clone, Copy)]
enum Kind {
	Join,
	Leave,
	Move,
	Mute,
	Unmute,
	Deafen,
	Undeafen,
}

impl Kind {
	const ALL: [Self; KINDS] = [
		Self::Join,
		Self::Leave,
		Self::Move,
		Self::Mute,
		Self::Unmute,
		Self::Deafen,
		Self::Undeafen,
	];

	fn key(self) -> &'static str {
		match self {
			Self::Join => "joinMessage",
			Self::Leave => "leaveMessage",
			Self::Move => "moveMessage",
			Self::Mute => "muteMessage",
			Self::Unmute => "unmuteMessage",
			Self::Deafen => "deafenMessage",
			Self::Undeafen => "undeafenMessage",
		}
	}

	fn template(self) -> &'static str {
		match self {
			Self::Join => "{{USER}} joined",
			Self::Leave => "{{USER}} left",
			Self::Move => "{{USER}} moved to {{CHANNEL}}",
			Self::Mute => "{{USER}} muted",
			Self::Unmute => "{{USER}} unmuted",
			Self::Deafen => "{{USER}} deafened",
			Self::Undeafen => "{{USER}} undeafened",
		}
	}
}

struct Config {
	volume: f32,
	rate: f32,
	say_own_name: bool,
	latin_only: bool,
	templates: [String; KINDS],
}

impl Default for Config {
	fn default() -> Self {
		Self {
			volume: 1.,
			rate: 1.,
			say_own_name: false,
			latin_only: false,
			templates: Kind::ALL.map(|kind| kind.template().to_owned()),
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	matches!(raw.trim(), "true" | "1" | "on")
}

fn is_latin(letter: char) -> bool {
	letter.is_ascii_alphabetic() || matches!(letter, '\u{C0}'..='\u{24F}' | '\u{1E00}'..='\u{1EFF}')
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			"volume" => {
				self.volume = value
					.trim()
					.parse()
					.map_or(1., |volume: f32| volume.clamp(0., 1.))
			}
			"rate" => {
				self.rate = value
					.trim()
					.parse()
					.map_or(1., |rate: f32| rate.clamp(MIN_RATE, MAX_RATE));
			}
			"sayOwnName" => self.say_own_name = parse_flag(value),
			"latinOnly" => self.latin_only = parse_flag(value),
			_ => {
				if let Some(kind) = Kind::ALL.iter().find(|kind| kind.key() == key) {
					self.templates[*kind as usize] = value.to_owned();
				}
			}
		}
	}

	fn clean(&self, text: &str) -> String {
		let mut cleaned = String::with_capacity(text.len());
		for letter in text.chars() {
			let letter_allowed = if self.latin_only {
				is_latin(letter)
			} else {
				letter.is_alphabetic()
			};
			let allowed = letter_allowed
				|| letter.is_numeric()
				|| letter.is_whitespace()
				|| letter.is_ascii_punctuation();
			if allowed && !(letter == '_' && cleaned.ends_with('_')) {
				cleaned.push(letter);
			}
		}
		cleaned.trim().to_owned()
	}

	fn sentence(&self, kind: Kind, user: &str, channel: &str) -> String {
		let mut spoken_user = self.clean(user);
		if spoken_user.is_empty() && !user.is_empty() {
			"Someone".clone_into(&mut spoken_user);
		}
		let mut spoken_channel = self.clean(channel);
		if spoken_channel.is_empty() {
			"channel".clone_into(&mut spoken_channel);
		}
		self.templates[kind as usize]
			.replace("{{USER}}", &spoken_user)
			.replace("{{DISPLAY_NAME}}", &spoken_user)
			.replace("{{NICKNAME}}", &spoken_user)
			.replace("{{CHANNEL}}", &spoken_channel)
			.trim()
			.to_owned()
	}
}

fn others(from: &VoiceInfo, against: &VoiceInfo, kind: Kind, me: u64) -> impl Iterator<Item = (Kind, u64)> {
	from.participants
		.iter()
		.filter(move |id| **id != me && !against.participants.contains(id))
		.map(move |id| (kind, *id))
}

fn changes(previous: Option<&VoiceInfo>, current: &VoiceInfo, me: u64) -> Vec<(Kind, u64)> {
	let Some(previous) = previous else {
		return vec![(Kind::Join, me)];
	};
	if previous.channel_id != current.channel_id {
		return vec![(Kind::Move, me)];
	}
	let mut found: Vec<(Kind, u64)> = others(current, previous, Kind::Join, me)
		.chain(others(previous, current, Kind::Leave, me))
		.collect();
	if previous.self_mute != current.self_mute {
		found.push((
			if current.self_mute {
				Kind::Mute
			} else {
				Kind::Unmute
			},
			me,
		));
	}
	if previous.self_deaf != current.self_deaf {
		found.push((
			if current.self_deaf {
				Kind::Deafen
			} else {
				Kind::Undeafen
			},
			me,
		));
	}
	found
}

#[derive(Default)]
struct VcNarrator {
	config: Config,
	me: Option<u64>,
	call: Option<VoiceInfo>,
	channel_name: String,
	watcher: Option<u64>,
}

impl VcNarrator {
	fn own_id(&mut self) -> Option<u64> {
		if self.me.is_none() {
			self.me = host::current_user().ok().map(|user| user.id);
		}
		self.me
	}

	fn name_of(&self, guild_id: Option<u64>, user_id: u64, me: u64) -> String {
		if user_id == me && !self.config.say_own_name {
			return String::new();
		}
		host::display_name(guild_id, user_id).unwrap_or_default()
	}

	fn say(&self, lines: &[String]) {
		let text: String = lines.join(". ").chars().take(MAX_SPOKEN_CHARS).collect();
		if text.trim().is_empty() {
			return;
		}
		if let Err(error) = actions::speak(&text, self.config.rate, self.config.volume) {
			log::debug!("could not speak: {error:?}");
		}
	}

	fn watch(&mut self) {
		if self.watcher.is_some() {
			return;
		}
		match timer::every(WATCH_MS, WATCH_TAG) {
			Ok(id) => self.watcher = Some(id),
			Err(error) => log::warn!("could not watch for the end of the call: {error:?}"),
		}
	}

	fn announce(&mut self, info: VoiceInfo) {
		let Some(me) = self.own_id() else {
			return;
		};
		let found = changes(self.call.as_ref(), &info, me);
		let guild_id = info.guild_id;
		self.call = Some(info);
		if found.is_empty() {
			return;
		}
		if let Ok(Some(detail)) = data::voice_detail() {
			self.channel_name = detail.channel_name;
		}
		let lines: Vec<String> = found
			.into_iter()
			.map(|(kind, user_id)| {
				let user = self.name_of(guild_id, user_id, me);
				self.config.sentence(kind, &user, &self.channel_name)
			})
			.collect();
		self.say(&lines);
	}

	fn finish_call(&mut self) {
		if let Some(id) = self.watcher.take() {
			timer::clear(id);
		}
		let Some(previous) = self.call.take() else {
			return;
		};
		if let Some(me) = self.own_id() {
			let user = self.name_of(previous.guild_id, me, me);
			self.say(&[self.config.sentence(Kind::Leave, &user, &self.channel_name)]);
		}
	}
}

impl Plugin for VcNarrator {
	fn init(&mut self) -> Result<(), String> {
		for key in storage::keys().unwrap_or_default() {
			if let Some(setting) = key.strip_prefix("settings.")
				&& let Ok(Some(saved)) = storage::get(&key)
			{
				self.config.set(setting, &String::from_utf8_lossy(&saved));
			}
		}
		if let Ok(Some(detail)) = data::voice_detail() {
			self.channel_name = detail.channel_name;
			self.call = Some(detail.base);
			self.watch();
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::VoiceChanged(info) => {
				self.watch();
				self.announce(info);
			}
			Event::SettingChanged(SettingChange { key, value }) => self.config.set(&key, &value),
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"volume":{"type":"number","title":"Narrator volume","description":"Narrator Volume","minimum":0,"maximum":1,"default":1},"rate":{"type":"number","title":"Narrator speed","description":"Narrator Speed, between half and double speed","minimum":0.5,"maximum":2,"default":1},"sayOwnName":{"type":"boolean","title":"Say own name","description":"Say own name","default":false},"latinOnly":{"type":"boolean","title":"Latin only","description":"Strip non latin characters from names before saying them","default":false},"joinMessage":{"type":"string","title":"Join message","description":"Message when someone joins, {{USER}} and {{CHANNEL}} are replaced","default":"{{USER}} joined"},"leaveMessage":{"type":"string","title":"Leave message","description":"Message when someone leaves","default":"{{USER}} left"},"moveMessage":{"type":"string","title":"Move message","description":"Message when you move to another channel","default":"{{USER}} moved to {{CHANNEL}}"},"muteMessage":{"type":"string","title":"Mute message","description":"Mute Message (only self for now)","default":"{{USER}} muted"},"unmuteMessage":{"type":"string","title":"Unmute message","description":"Unmute Message (only self for now)","default":"{{USER}} unmuted"},"deafenMessage":{"type":"string","title":"Deafen message","description":"Deafen Message (only self for now)","default":"{{USER}} deafened"},"undeafenMessage":{"type":"string","title":"Undeafen message","description":"Undeafen Message (only self for now)","default":"{{USER}} undeafened"}}}"#
			.to_owned()
	}
}

impl PluginExt for VcNarrator {
	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == WATCH_TAG && matches!(data::voice_detail(), Ok(None)) {
			self.finish_call();
		}
	}
}

export_plugin!(VcNarrator, ext);
