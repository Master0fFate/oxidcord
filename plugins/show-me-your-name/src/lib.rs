use std::collections::HashMap;

use plugin_sdk::model::{ClientEvent, DisplayOverrideFull, Presence, Relationship, RelationshipKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, User, actions, data, export_plugin, log, storage};

const SETTING_KEYS: [&str; 3] = ["mode", "friendNicknames", "displayNames"];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
	#[default]
	UserNick,
	NickUser,
	UserOnly,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum FriendNicknames {
	#[default]
	Dms,
	Always,
	Fallback,
}

impl FriendNicknames {
	fn applies(self, in_dm: bool, has_server_nick: bool) -> bool {
		match self {
			Self::Dms => in_dm,
			Self::Always => true,
			Self::Fallback => !has_server_nick,
		}
	}
}

#[derive(Default)]
struct Config {
	mode: Mode,
	friend_nicknames: FriendNicknames,
	display_names: bool,
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let value = value.trim();
		match key {
			"mode" => {
				self.mode = match value {
					"nick-user" => Mode::NickUser,
					"user" => Mode::UserOnly,
					_ => Mode::UserNick,
				};
			}
			"friendNicknames" => {
				self.friend_nicknames = match value {
					"always" => FriendNicknames::Always,
					"fallback" => FriendNicknames::Fallback,
					_ => FriendNicknames::Dms,
				};
			}
			"displayNames" => self.display_names = matches!(value, "true" | "1" | "on"),
			_ => {}
		}
	}
}

#[derive(Default)]
struct ShowMeYourName {
	config: Config,
	nicknames: HashMap<u64, String>,
}

fn or_username(user: &User) -> &str {
	if user.display_name.is_empty() {
		&user.username
	} else {
		&user.display_name
	}
}

fn naming(name: String, suffix: Option<String>) -> DisplayOverrideFull {
	DisplayOverrideFull {
		name: Some(name),
		suffix,
		color: None,
		badges: Vec::new(),
		avatar: None,
		banner: None,
		decoration: None,
		pronouns: None,
	}
}

impl ShowMeYourName {
	fn remember(&mut self, relationship: &Relationship) {
		let nickname = relationship
			.nickname
			.as_ref()
			.filter(|_| relationship.kind == RelationshipKind::Friend);
		match nickname {
			Some(nickname) => {
				self.nicknames.insert(relationship.user.id, nickname.clone());
			}
			None => {
				self.nicknames.remove(&relationship.user.id);
			}
		}
	}

	fn load_nicknames(&mut self) {
		match data::list_relationships() {
			Ok(people) => {
				self.nicknames.clear();
				for relationship in &people {
					self.remember(relationship);
				}
			}
			Err(error) => log::warn!("could not read friend nicknames: {error:?}"),
		}
	}

	fn refresh(&self, user_id: u64) {
		if let Err(error) = actions::invalidate_user(user_id) {
			log::debug!("could not refresh user {user_id}: {error:?}");
		}
	}
}

impl Plugin for ShowMeYourName {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		self.load_nicknames();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"mode":{"type":"string","title":"Name order","description":"user-nick writes the username then the nickname, nick-user the other way round, user only the username","enum":["user-nick","nick-user","user"],"default":"user-nick"},"friendNicknames":{"type":"string","title":"Friend nicknames","description":"dms uses your friend nicknames only in direct messages, always prefers them over server nicknames, fallback only when there is no server nickname","enum":["dms","always","fallback"],"default":"dms"},"displayNames":{"type":"boolean","title":"Display names","description":"Use display names in place of usernames","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for ShowMeYourName {
	fn display_user_full(
		&mut self,
		guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		let username = if self.config.display_names {
			or_username(user)
		} else {
			&user.username
		};
		let server_nick = guild_id
			.and_then(|guild_id| data::member_info(guild_id, user.id).ok().flatten())
			.and_then(|member| member.nick)
			.filter(|nick| !nick.is_empty());
		let friend_nick = self.nicknames.get(&user.id).filter(|_| {
			self.config
				.friend_nicknames
				.applies(guild_id.is_none(), server_nick.is_some())
		});
		let nick = friend_nick
			.cloned()
			.or(server_nick)
			.unwrap_or_else(|| or_username(user).to_owned());
		if self.config.mode == Mode::UserOnly {
			return Some(naming(username.to_owned(), None));
		}
		if username.to_lowercase() == nick.to_lowercase() {
			return Some(naming(nick, None));
		}
		Some(match self.config.mode {
			Mode::NickUser => naming(nick, Some(username.to_owned())),
			_ => naming(username.to_owned(), Some(nick)),
		})
	}

	fn on_client_event(&mut self, event: ClientEvent) {
		match event {
			ClientEvent::RelationshipChanged(relationship) => {
				let before = self.nicknames.get(&relationship.user.id).cloned();
				self.remember(&relationship);
				if before.as_ref() != self.nicknames.get(&relationship.user.id) {
					self.refresh(relationship.user.id);
				}
			}
			ClientEvent::RelationshipRemoved(id) => {
				if self.nicknames.remove(&id).is_some() {
					self.refresh(id);
				}
			}
			ClientEvent::Ready => self.load_nicknames(),
			_ => {}
		}
	}
}

export_plugin!(ShowMeYourName, ext);
