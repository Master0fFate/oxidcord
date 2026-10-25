use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Instant;

use parking_lot::Mutex;
use protocol::{
	Channel, Emoji, GuildProperties, Member, Presence, ReadState, Relationship, Role, Snowflake, StatusKind,
	User, UserGuildSettings,
};
use serde_json::Value;
use tokio::sync::watch;

use crate::config::MockConfig;
use crate::fixtures::Fixtures;
use crate::hub::Hub;
use crate::ids::snowflake_at;

const WINDOW_SECONDS: f64 = 2.0;
const BUCKET_LIMIT: u32 = 5;

pub struct Urls {
	pub rest: String,
	pub gateway: String,
	pub voice: String,
}

pub struct Store {
	pub messages: BTreeMap<u64, BTreeMap<u64, Value>>,
	pub channels: HashMap<u64, Channel>,
	pub private_channel_ids: Vec<u64>,
	pub read_states: BTreeMap<u64, ReadState>,
	pub presences: HashMap<u64, HashMap<u64, Presence>>,
	pub current_user: User,
	pub profile: serde_json::Map<String, Value>,
	pub relationships: BTreeMap<u64, Relationship>,
	pub settings: BTreeMap<u64, UserGuildSettings>,
	pub properties: HashMap<u64, GuildProperties>,
	pub roles: HashMap<u64, Vec<Role>>,
	pub emojis: HashMap<u64, Vec<Emoji>>,
	pub removed_members: HashMap<u64, BTreeSet<u64>>,
	pub bans: HashMap<u64, BTreeMap<u64, Option<String>>>,
	pub member_edits: HashMap<(u64, u64), Member>,
	pub invites: BTreeMap<String, Value>,
	pub left_guilds: BTreeSet<u64>,
	pub audit: HashMap<u64, Vec<Value>>,
	pub joined_threads: BTreeSet<u64>,
	pub created_guilds: HashMap<u64, Value>,
	pub gif_favorites: Vec<Value>,
	last_id: u64,
}

impl Store {
	pub fn new(fixtures: &Fixtures) -> Self {
		let last_id = fixtures
			.messages
			.values()
			.filter_map(|messages| messages.keys().next_back())
			.copied()
			.max()
			.unwrap_or_default();
		Self {
			messages: fixtures.messages.clone(),
			channels: fixtures
				.channels
				.iter()
				.map(|(id, channel)| (*id, channel.clone()))
				.collect(),
			private_channel_ids: fixtures.private_channel_ids.clone(),
			read_states: fixtures
				.read_states
				.iter()
				.map(|state| (state.id.0, state.clone()))
				.collect(),
			presences: fixtures
				.guilds
				.iter()
				.map(|guild| {
					let by_user = guild
						.presences
						.iter()
						.filter_map(|presence| presence.user_id.map(|id| (id.0, presence.clone())))
						.collect();
					(guild.id, by_user)
				})
				.collect(),
			current_user: fixtures.current_user.clone(),
			profile: serde_json::Map::new(),
			relationships: fixtures
				.relationships
				.iter()
				.map(|relationship| (relationship.id.0, relationship.clone()))
				.collect(),
			settings: fixtures
				.user_guild_settings
				.iter()
				.map(|settings| (settings.guild_id.map_or(0, |id| id.0), settings.clone()))
				.collect(),
			properties: fixtures
				.guilds
				.iter()
				.map(|guild| (guild.id, guild.properties.clone()))
				.collect(),
			roles: fixtures
				.guilds
				.iter()
				.map(|guild| (guild.id, guild.roles.clone()))
				.collect(),
			emojis: fixtures
				.guilds
				.iter()
				.map(|guild| (guild.id, guild.emojis.clone()))
				.collect(),
			removed_members: HashMap::new(),
			bans: HashMap::new(),
			member_edits: HashMap::new(),
			invites: BTreeMap::new(),
			created_guilds: HashMap::new(),
			left_guilds: BTreeSet::new(),
			audit: crate::admin::seed_audit(fixtures),
			joined_threads: BTreeSet::new(),
			gif_favorites: Vec::new(),
			last_id,
		}
	}

	pub fn next_snowflake(&mut self) -> u64 {
		let now_ms = chrono::Utc::now().timestamp_millis();
		self.last_id = snowflake_at(now_ms, 0).max(self.last_id + 1);
		self.last_id
	}

	pub fn newest_message_id(&self, channel_id: u64) -> Option<u64> {
		self.messages.get(&channel_id)?.keys().next_back().copied()
	}

	pub fn refresh_last_message(&mut self, channel_id: u64) {
		let newest = self.newest_message_id(channel_id).map(Snowflake);
		if let Some(channel) = self.channels.get_mut(&channel_id) {
			channel.last_message_id = newest;
		}
	}

	pub fn online_count(&self, guild_id: u64) -> usize {
		self.presences.get(&guild_id).map_or(0, |by_user| {
			by_user
				.values()
				.filter(|presence| presence.status != StatusKind::Offline)
				.count()
		})
	}

	pub fn message(&self, channel_id: u64, id: u64) -> Option<&Value> {
		self.messages.get(&channel_id)?.get(&id)
	}

	pub fn message_mut(&mut self, channel_id: u64, id: u64) -> Option<&mut Value> {
		self.messages.get_mut(&channel_id)?.get_mut(&id)
	}
}

struct Window {
	reset_at: Instant,
	used: u32,
}

pub struct RateLimitSnapshot {
	pub remaining: u32,
	pub reset_after: f64,
}

pub struct Shared {
	pub config: MockConfig,
	pub fixtures: Fixtures,
	pub store: Mutex<Store>,
	pub hub: Hub,
	pub urls: Urls,
	pub shutdown: watch::Receiver<bool>,
	windows: Mutex<HashMap<String, Window>>,
}

impl Shared {
	pub fn new(config: MockConfig, fixtures: Fixtures, urls: Urls, shutdown: watch::Receiver<bool>) -> Self {
		let store = Mutex::new(Store::new(&fixtures));
		Self {
			config,
			fixtures,
			store,
			hub: Hub::default(),
			urls,
			shutdown,
			windows: Mutex::new(HashMap::new()),
		}
	}

	pub fn touch_bucket(&self, key: &str) -> RateLimitSnapshot {
		let now = Instant::now();
		let mut windows = self.windows.lock();
		let window = windows.entry(key.to_owned()).or_insert(Window {
			reset_at: now,
			used: 0,
		});
		if window.reset_at <= now {
			window.reset_at = now + std::time::Duration::from_secs_f64(WINDOW_SECONDS);
			window.used = 0;
		}
		window.used += 1;
		RateLimitSnapshot {
			remaining: BUCKET_LIMIT.saturating_sub(window.used),
			reset_after: window.reset_at.saturating_duration_since(now).as_secs_f64(),
		}
	}
}
