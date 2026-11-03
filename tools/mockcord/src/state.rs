use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use protocol::{
	Channel, Emoji, FRIEND_SOURCE_MUTUAL_FRIENDS, FRIEND_SOURCE_MUTUAL_GUILDS, GuildProperties, Member,
	Presence, ProtoCustomStatus, ProtoFrecencySettings, ProtoStatus, ReadState, Relationship, Role,
	Snowflake, StatusKind, User, UserGuildSettings, encode_status, privacy_with_friend_sources,
};
use serde_json::Value;
use tokio::sync::{broadcast, watch};

use crate::config::MockConfig;
use crate::fixtures::Fixtures;
use crate::hub::Hub;
use crate::ids::snowflake_at;
use crate::scenario::VoiceJoin;
use crate::voice::Signal;

const WINDOW_SECONDS: f64 = 2.0;
const BUCKET_LIMIT: u32 = 5;
const MOCK_STATUS_MINUTES: i64 = 30;

fn seed_status() -> Vec<u8> {
	let expires = chrono::Utc::now() + chrono::Duration::minutes(MOCK_STATUS_MINUTES);
	encode_status(&ProtoStatus {
		status: Some("online".to_owned()),
		custom_status: Some(ProtoCustomStatus {
			text: "mock status".to_owned(),
			emoji_id: None,
			emoji_name: Some("\u{1f3a7}".to_owned()),
			expires_at_ms: u64::try_from(expires.timestamp_millis()).ok(),
		}),
		show_current_game: true,
	})
}

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
	pub notes: BTreeMap<u64, String>,
	pub settings: BTreeMap<u64, UserGuildSettings>,
	pub properties: HashMap<u64, GuildProperties>,
	pub roles: HashMap<u64, Vec<Role>>,
	pub emojis: HashMap<u64, Vec<Emoji>>,
	pub removed_members: HashMap<u64, BTreeSet<u64>>,
	pub bans: HashMap<u64, BTreeMap<u64, Option<String>>>,
	pub member_edits: HashMap<(u64, u64), Member>,
	pub invites: BTreeMap<String, Value>,
	pub left_guilds: BTreeSet<u64>,
	pub ended_sessions: BTreeSet<String>,
	pub guild_bios: HashMap<(u64, u64), String>,
	pub audit: HashMap<u64, Vec<Value>>,
	pub joined_threads: BTreeSet<u64>,
	pub slowmode: HashMap<u64, Instant>,
	pub pinned_at: HashMap<u64, DateTime<Utc>>,
	pub created_guilds: HashMap<u64, Value>,
	pub frecency: ProtoFrecencySettings,
	pub proto_status: Vec<u8>,
	pub proto_privacy: Vec<u8>,
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
			notes: BTreeMap::new(),
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
			ended_sessions: BTreeSet::new(),
			guild_bios: HashMap::new(),
			audit: crate::admin::seed_audit(fixtures),
			pinned_at: fixtures.pinned_at(),
			joined_threads: fixtures
				.guilds
				.first()
				.and_then(|guild| guild.thread_ids.first().copied())
				.into_iter()
				.collect(),
			slowmode: HashMap::new(),
			proto_status: seed_status(),
			proto_privacy: privacy_with_friend_sources(
				&[],
				FRIEND_SOURCE_MUTUAL_FRIENDS | FRIEND_SOURCE_MUTUAL_GUILDS,
			),
			frecency: ProtoFrecencySettings {
				data_version: 1,
				..ProtoFrecencySettings::default()
			},
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
	pub voice_signals: broadcast::Sender<Signal>,
	pub voice_join: Mutex<Option<VoiceJoin>>,
	pub voice_stale: AtomicBool,
	windows: Mutex<HashMap<String, Window>>,
	indexed: AtomicBool,
}

impl Shared {
	pub fn new(
		config: MockConfig,
		fixtures: Fixtures,
		urls: Urls,
		voice_signals: broadcast::Sender<Signal>,
		shutdown: watch::Receiver<bool>,
	) -> Self {
		let store = Mutex::new(Store::new(&fixtures));
		Self {
			config,
			fixtures,
			store,
			hub: Hub::default(),
			urls,
			shutdown,
			voice_signals,
			voice_join: Mutex::new(None),
			voice_stale: AtomicBool::new(false),
			windows: Mutex::new(HashMap::new()),
			indexed: AtomicBool::new(false),
		}
	}

	pub fn index_pending(&self) -> bool {
		self.config.chaos && !self.indexed.swap(true, Ordering::Relaxed)
	}

	pub async fn simulate_latency(&self) {
		if self.config.latency_ms > 0 {
			tokio::time::sleep(Duration::from_millis(self.config.latency_ms)).await;
		}
	}

	pub fn account_for_token(&self, token: &str) -> Option<User> {
		if self.config.accepts(token) {
			Some(self.store.lock().current_user.clone())
		} else if self.config.accepts_second(token) {
			Some(self.fixtures.second_user.clone())
		} else {
			None
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
