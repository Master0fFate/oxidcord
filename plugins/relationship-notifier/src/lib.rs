use std::collections::BTreeMap;

use plugin_sdk::model::{ClientEvent, Relationship, RelationshipKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{Event, Plugin, PluginExt, data, export_plugin, host, log, mount, storage, timer};

const CACHE_KEY: &str = "cache";
const SYNC_TAG: &str = "sync";
const FLUSH_TAG: &str = "flush";
const SYNC_DELAY_MS: u32 = 5_000;
const FLUSH_DELAY_MS: u32 = 1_000;
const NOTICE_MS: u32 = 8_000;
const MAX_LINES: usize = 5;
const GROUP_DM_KIND: u32 = 3;
const SETTING_KEYS: [&str; 6] = [
	"notices",
	"offlineRemovals",
	"friends",
	"friendRequestCancels",
	"servers",
	"groups",
];

struct Config {
	notices: bool,
	offline_removals: bool,
	friends: bool,
	friend_request_cancels: bool,
	servers: bool,
	groups: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			notices: false,
			offline_removals: true,
			friends: true,
			friend_request_cancels: true,
			servers: true,
			groups: true,
		}
	}
}

impl Config {
	fn set(&mut self, key: &str, value: &str) {
		let on = matches!(value.trim(), "true" | "1" | "on");
		match key {
			"notices" => self.notices = on,
			"offlineRemovals" => self.offline_removals = on,
			"friends" => self.friends = on,
			"friendRequestCancels" => self.friend_request_cancels = on,
			"servers" => self.servers = on,
			"groups" => self.groups = on,
			_ => {}
		}
	}
}

type Names = BTreeMap<u64, String>;

#[derive(Default, PartialEq)]
struct Snapshot {
	friends: Names,
	requests: Names,
	guilds: Names,
	groups: Names,
}

impl Snapshot {
	fn is_empty(&self) -> bool {
		self.friends.is_empty() && self.guilds.is_empty() && self.groups.is_empty()
	}

	fn encode(&self) -> String {
		let mut out = String::new();
		for (tag, names) in [
			('f', &self.friends),
			('r', &self.requests),
			('g', &self.guilds),
			('c', &self.groups),
		] {
			for (id, name) in names {
				let clean = name.replace(['\t', '\n', '\r'], " ");
				out.push_str(&format!("{tag}\t{id}\t{clean}\n"));
			}
		}
		out
	}

	fn decode(raw: &str) -> Self {
		let mut snapshot = Self::default();
		for line in raw.lines() {
			let mut parts = line.splitn(3, '\t');
			let (Some(tag), Some(id), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
				continue;
			};
			let Ok(id) = id.parse::<u64>() else {
				continue;
			};
			let names = match tag {
				"f" => &mut snapshot.friends,
				"r" => &mut snapshot.requests,
				"g" => &mut snapshot.guilds,
				"c" => &mut snapshot.groups,
				_ => continue,
			};
			names.insert(id, name.to_owned());
		}
		snapshot
	}

	fn read_host() -> Self {
		let mut snapshot = Self::default();
		for relationship in data::list_relationships().unwrap_or_default() {
			let names = match relationship.kind {
				RelationshipKind::Friend => &mut snapshot.friends,
				RelationshipKind::PendingIncoming => &mut snapshot.requests,
				_ => continue,
			};
			names.insert(relationship.user.id, relationship.user.display_name);
		}
		snapshot.guilds = guild_names();
		snapshot.groups = group_names();
		snapshot
	}
}

fn guild_names() -> Names {
	host::list_guilds()
		.unwrap_or_default()
		.into_iter()
		.map(|guild| (guild.id, guild.name))
		.collect()
}

fn group_names() -> Names {
	host::list_channels(None)
		.unwrap_or_default()
		.into_iter()
		.filter(|channel| channel.kind == GROUP_DM_KIND)
		.map(|channel| (channel.id, channel.name))
		.collect()
}

fn gone<'a>(known: &'a Names, present: impl Fn(u64) -> bool + 'a) -> impl Iterator<Item = &'a str> + 'a {
	known
		.iter()
		.filter(move |(id, _)| !present(**id))
		.map(|(_, name)| name.as_str())
}

#[derive(Default)]
struct RelationshipNotifier {
	config: Config,
	known: Snapshot,
	pending: Vec<String>,
	flush_queued: bool,
}

impl RelationshipNotifier {
	fn save(&self) {
		if let Err(error) = storage::set(CACHE_KEY, self.known.encode().as_bytes()) {
			log::warn!("could not save the name cache: {error:?}");
		}
	}

	fn say(&mut self, line: String) {
		self.pending.push(line);
		if self.flush_queued {
			return;
		}
		match timer::once(FLUSH_DELAY_MS, FLUSH_TAG) {
			Ok(_) => self.flush_queued = true,
			Err(error) => log::warn!("could not schedule the notification: {error:?}"),
		}
	}

	fn flush(&mut self) {
		self.flush_queued = false;
		if self.pending.is_empty() {
			return;
		}
		let hidden = self.pending.len().saturating_sub(MAX_LINES);
		self.pending.truncate(MAX_LINES);
		if hidden > 0 {
			self.pending.push(format!("and {hidden} more"));
		}
		let body = self.pending.join("\n");
		self.pending.clear();
		if let Err(error) = host::notify("Removed", &body) {
			log::debug!("notification skipped: {error:?}");
		}
		if self.config.notices
			&& let Err(error) = mount::toast([view::text(body)], NOTICE_MS)
		{
			log::debug!("notice skipped: {error:?}");
		}
	}

	fn sync_and_check(&mut self) {
		let current = Snapshot::read_host();
		if current.is_empty() {
			return;
		}
		if self.config.offline_removals {
			self.report_missing(&current);
		}
		if current != self.known {
			self.known = current;
			self.save();
		}
	}

	fn report_missing(&mut self, current: &Snapshot) {
		let known = &self.known;
		let mut lines = Vec::new();
		if self.config.friends {
			lines.extend(
				gone(&known.friends, |id| current.friends.contains_key(&id))
					.map(|name| format!("{name} is no longer your friend")),
			);
		}
		if self.config.friend_request_cancels {
			lines.extend(
				gone(&known.requests, |id| {
					current.requests.contains_key(&id) || current.friends.contains_key(&id)
				})
				.map(|name| format!("Request from {name} is gone")),
			);
		}
		if self.config.servers {
			lines.extend(
				gone(&known.guilds, |id| current.guilds.contains_key(&id))
					.map(|name| format!("No longer in server {name}")),
			);
		}
		if self.config.groups {
			lines.extend(
				gone(&known.groups, |id| current.groups.contains_key(&id))
					.map(|name| format!("No longer in group {name}")),
			);
		}
		for line in lines {
			self.say(line);
		}
	}

	fn on_relationship(&mut self, relationship: Relationship) {
		let id = relationship.user.id;
		self.known.friends.remove(&id);
		self.known.requests.remove(&id);
		let names = match relationship.kind {
			RelationshipKind::Friend => &mut self.known.friends,
			RelationshipKind::PendingIncoming => &mut self.known.requests,
			_ => return self.save(),
		};
		names.insert(id, relationship.user.display_name);
		self.save();
	}

	fn on_relationship_removed(&mut self, id: u64) {
		let line = if let Some(name) = self.known.friends.remove(&id) {
			self.config
				.friends
				.then(|| format!("{name} removed you as a friend"))
		} else if let Some(name) = self.known.requests.remove(&id) {
			self.config
				.friend_request_cancels
				.then(|| format!("{name} withdrew a friend request"))
		} else {
			return;
		};
		self.save();
		if let Some(line) = line {
			self.say(line);
		}
	}

	fn on_guild_left(&mut self, id: u64) {
		let Some(name) = self.known.guilds.remove(&id) else {
			return;
		};
		self.save();
		if self.config.servers {
			self.say(format!("You were removed from {name}"));
		}
	}

	fn on_group_closed(&mut self, id: u64) {
		let Some(name) = self.known.groups.remove(&id) else {
			return;
		};
		self.save();
		if self.config.groups {
			self.say(format!("You were removed from the group {name}"));
		}
	}

	fn refresh_names(&mut self) {
		let guilds = guild_names();
		let groups = group_names();
		if guilds.is_empty() && groups.is_empty() {
			return;
		}
		if guilds != self.known.guilds || groups != self.known.groups {
			self.known.guilds = guilds;
			self.known.groups = groups;
			self.save();
		}
	}
}

impl Plugin for RelationshipNotifier {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(CACHE_KEY) {
			self.known = Snapshot::decode(&String::from_utf8_lossy(&saved));
		}
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		if let Err(error) = timer::once(SYNC_DELAY_MS, SYNC_TAG) {
			log::warn!("could not schedule the startup check: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event {
			self.config.set(&key, &value);
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"notices":{"type":"boolean","title":"Also show a notice","description":"Also show a notice on screen when something was removed","default":false},"offlineRemovals":{"type":"boolean","title":"Removals while offline","description":"Notify you at startup if you were removed while the app was closed","default":true},"friends":{"type":"boolean","title":"Friends","description":"Notify when a friend removes you","default":true},"friendRequestCancels":{"type":"boolean","title":"Cancelled requests","description":"Notify when a friend request is cancelled","default":true},"servers":{"type":"boolean","title":"Servers","description":"Notify when removed from a server","default":true},"groups":{"type":"boolean","title":"Group chats","description":"Notify when removed from a group chat","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for RelationshipNotifier {
	fn on_client_event(&mut self, event: ClientEvent) {
		match event {
			ClientEvent::RelationshipChanged(relationship) => self.on_relationship(relationship),
			ClientEvent::RelationshipRemoved(id) => self.on_relationship_removed(id),
			ClientEvent::GuildLeft(id) => self.on_guild_left(id),
			ClientEvent::DmClosed(id) => self.on_group_closed(id),
			ClientEvent::GuildJoined(_) | ClientEvent::ChannelListChanged => self.refresh_names(),
			ClientEvent::Ready => self.sync_and_check(),
			_ => {}
		}
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		match tag {
			SYNC_TAG => self.sync_and_check(),
			FLUSH_TAG => self.flush(),
			_ => {}
		}
	}
}

export_plugin!(RelationshipNotifier, ext);
