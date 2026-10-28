use std::collections::{HashMap, HashSet};

use plugin_sdk::model::{Badge, ChannelDetail, ClientEvent, RelationshipKind};
use plugin_sdk::types::{SettingChange, TypingEvent};
use plugin_sdk::{Event, Plugin, PluginExt, actions, data, export_plugin, host, log, now_ms, storage, timer};

const TYPING_MS: i64 = 10_000;
const MIN_TIMER_MS: i64 = 250;
const EXPIRE_TAG: &str = "expire";
const TOOLTIP_NAMES: usize = 3;
const SETTING_KEYS: [&str; 4] = [
	"includeCurrentChannel",
	"includeMutedChannels",
	"includeBlockedUsers",
	"indicatorMode",
];

struct Config {
	include_current: bool,
	include_muted: bool,
	include_blocked: bool,
	show_count: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			include_current: true,
			include_muted: false,
			include_blocked: false,
			show_count: false,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let on = matches!(value.trim(), "true" | "1" | "on");
		match key {
			"includeCurrentChannel" => self.include_current = on,
			"includeMutedChannels" => self.include_muted = on,
			"includeBlockedUsers" => self.include_blocked = on,
			"indicatorMode" => self.show_count = value.trim() == "count",
			_ => {}
		}
	}
}

#[derive(Default)]
struct TypingIndicator {
	config: Config,
	typing: HashMap<u64, Vec<(u64, i64)>>,
	blocked: HashSet<u64>,
	me: Option<u64>,
	timer_armed: bool,
}

impl TypingIndicator {
	fn refresh_channel(&self, channel_id: u64) {
		if let Err(error) = actions::invalidate_channel(channel_id) {
			log::debug!("could not refresh channel {channel_id}: {error:?}");
		}
	}

	fn refresh_all(&self) {
		for channel_id in self.typing.keys() {
			self.refresh_channel(*channel_id);
		}
	}

	fn arm_timer(&mut self) {
		if self.timer_armed {
			return;
		}
		let Some(earliest) = self.typing.values().flatten().map(|(_, expires)| *expires).min() else {
			return;
		};
		let delay = (earliest - now_ms()).max(MIN_TIMER_MS);
		match timer::once(u32::try_from(delay).unwrap_or(u32::MAX), EXPIRE_TAG) {
			Ok(_) => self.timer_armed = true,
			Err(error) => log::warn!("could not schedule the expiry: {error:?}"),
		}
	}

	fn expire(&mut self) {
		self.timer_armed = false;
		let now = now_ms();
		let mut changed = Vec::new();
		self.typing.retain(|channel_id, users| {
			let before = users.len();
			users.retain(|(_, expires)| *expires > now);
			if users.len() != before {
				changed.push(*channel_id);
			}
			!users.is_empty()
		});
		for channel_id in changed {
			self.refresh_channel(channel_id);
		}
		self.arm_timer();
	}

	fn on_typing(&mut self, typing: TypingEvent) {
		if self.me.is_none() {
			self.me = host::current_user().ok().map(|user| user.id);
		}
		if self.me == Some(typing.user_id) {
			return;
		}
		let expires = now_ms() + TYPING_MS;
		let users = self.typing.entry(typing.channel_id).or_default();
		match users.iter_mut().find(|(user_id, _)| *user_id == typing.user_id) {
			Some(entry) => entry.1 = expires,
			None => users.push((typing.user_id, expires)),
		}
		self.refresh_channel(typing.channel_id);
		self.arm_timer();
	}

	fn on_message(&mut self, channel_id: u64, user_id: u64) {
		let Some(users) = self.typing.get_mut(&channel_id) else {
			return;
		};
		let before = users.len();
		users.retain(|(typer, _)| *typer != user_id);
		if users.len() == before {
			return;
		}
		if users.is_empty() {
			self.typing.remove(&channel_id);
		}
		self.refresh_channel(channel_id);
	}

	fn visible_typers(&self, channel_id: u64) -> Vec<u64> {
		self.typing
			.get(&channel_id)
			.into_iter()
			.flatten()
			.map(|(user_id, _)| *user_id)
			.filter(|user_id| self.config.include_blocked || !self.blocked.contains(user_id))
			.collect()
	}

	fn is_hidden(&self, channel_id: u64) -> bool {
		if !self.config.include_muted && data::channel_muted(channel_id).unwrap_or(false) {
			return true;
		}
		!self.config.include_current
			&& matches!(host::current_channel(), Ok(Some((_, current))) if current == channel_id)
	}

	fn load_blocked(&mut self) {
		match data::list_relationships() {
			Ok(people) => {
				self.blocked = people
					.into_iter()
					.filter(|relationship| relationship.kind == RelationshipKind::Blocked)
					.map(|relationship| relationship.user.id)
					.collect();
			}
			Err(error) => log::warn!("could not read blocked users: {error:?}"),
		}
	}
}

fn tooltip(guild_id: Option<u64>, typers: &[u64]) -> String {
	if typers.len() > TOOLTIP_NAMES {
		return "Several people are typing".to_owned();
	}
	let names: Vec<String> = typers
		.iter()
		.map(|user_id| host::display_name(guild_id, *user_id).unwrap_or_else(|_| "Someone".to_owned()))
		.collect();
	match names.as_slice() {
		[only] => format!("{only} is typing"),
		[first, second] => format!("{first} and {second} are typing"),
		[first, second, third] => format!("{first}, {second} and {third} are typing"),
		_ => String::new(),
	}
}

impl Plugin for TypingIndicator {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		self.load_blocked();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::Typing(typing) => self.on_typing(typing),
			Event::MessageCreated(message) => self.on_message(message.channel_id, message.author.id),
			Event::ChannelSwitched(_) if !self.config.include_current => self.refresh_all(),
			Event::SettingChanged(SettingChange { key, value }) => {
				self.config.set(&key, &value);
				self.refresh_all();
			}
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"includeCurrentChannel":{"type":"boolean","title":"Current channel","description":"Show the indicator for the channel you are in","default":true},"includeMutedChannels":{"type":"boolean","title":"Muted channels","description":"Show the indicator for muted channels","default":false},"includeBlockedUsers":{"type":"boolean","title":"Blocked users","description":"Show the indicator for users you blocked","default":false},"indicatorMode":{"type":"string","title":"Indicator","description":"icon shows a dots icon, count adds the number of people typing","enum":["icon","count"],"default":"icon"}}}"#
			.to_owned()
	}
}

impl PluginExt for TypingIndicator {
	fn decorate_channel(&mut self, channel: &ChannelDetail) -> Vec<Badge> {
		let typers = self.visible_typers(channel.id);
		if typers.is_empty() || self.is_hidden(channel.id) {
			return Vec::new();
		}
		vec![Badge {
			icon: Some("ellipsis".to_owned()),
			image: None,
			label: self.config.show_count.then(|| typers.len().to_string()),
			tooltip: tooltip(channel.guild_id, &typers),
			color: None,
			key: None,
		}]
	}

	fn on_client_event(&mut self, event: ClientEvent) {
		match event {
			ClientEvent::RelationshipChanged(relationship) => {
				if relationship.kind == RelationshipKind::Blocked {
					self.blocked.insert(relationship.user.id);
				} else {
					self.blocked.remove(&relationship.user.id);
				}
			}
			ClientEvent::RelationshipRemoved(id) => {
				self.blocked.remove(&id);
			}
			ClientEvent::Ready => self.load_blocked(),
			_ => {}
		}
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == EXPIRE_TAG {
			self.expire();
		}
	}
}

export_plugin!(TypingIndicator, ext);
