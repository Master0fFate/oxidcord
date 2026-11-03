use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use protocol::{
	AutoModRule, Channel, Emoji, FRIEND_SOURCE_MUTUAL_FRIENDS, FRIEND_SOURCE_MUTUAL_GUILDS, GuildProperties,
	Member, Presence, PrivacyChange, ProtoCustomStatus, ProtoFrecencySettings, ProtoStatus, ReadState,
	Relationship, Role, ScheduledEvent, Snowflake, SoundboardSound, StageInstance, StatusKind, User,
	UserGuildSettings, encode_sensitive_media, encode_status, privacy_with,
};
use serde_json::Value;
use tokio::sync::{broadcast, watch};

use crate::config::MockConfig;
use crate::fixtures::{Fixtures, GuildOrigin};
use crate::hub::Hub;
use crate::ids::snowflake_at;
use crate::ready::MOCK_SENSITIVE_MEDIA;
use crate::scenario::VoiceJoin;
use crate::stage::Seat;
use crate::uploads::SendFault;
use crate::voice::Signal;

const WINDOW_SECONDS: f64 = 2.0;
const BUCKET_LIMIT: u32 = 5;
const MOCK_STATUS_MINUTES: i64 = 30;

fn seed_privacy(fixtures: &Fixtures) -> Vec<u8> {
	let friends = privacy_with(
		&[],
		PrivacyChange::FriendSources(FRIEND_SOURCE_MUTUAL_FRIENDS | FRIEND_SOURCE_MUTUAL_GUILDS),
	);
	match fixtures.guilds.get(1) {
		Some(guild) => privacy_with(
			&friends,
			PrivacyChange::GuildDms {
				guild_id: Snowflake(guild.id),
				restricted: true,
			},
		),
		None => friends,
	}
}

fn seed_connections() -> Vec<Value> {
	[
		("github", "7021", "oxid-dev", true, 1, false),
		("spotify", "31ab9", "oxid.listens", true, 1, true),
		("twitch", "58213", "oxid_live", false, 0, false),
	]
	.into_iter()
	.map(|(kind, id, name, verified, visibility, show_activity)| {
		serde_json::json!({
			"type": kind,
			"id": id,
			"name": name,
			"verified": verified,
			"visibility": visibility,
			"show_activity": show_activity,
			"friend_sync": false,
			"revoked": false,
			"metadata_visibility": 0,
			"two_way_link": false,
		})
	})
	.collect()
}

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
	pub scheduled_events: HashMap<u64, Vec<ScheduledEvent>>,
	pub event_interest: BTreeSet<u64>,
	pub removed_members: HashMap<u64, BTreeSet<u64>>,
	pub bans: HashMap<u64, BTreeMap<u64, Option<String>>>,
	pub automod_rules: HashMap<u64, Vec<AutoModRule>>,
	pub member_edits: HashMap<(u64, u64), Member>,
	pub invites: BTreeMap<String, Value>,
	pub absent_guilds: BTreeSet<u64>,
	pub ended_sessions: BTreeSet<String>,
	pub dismissed_mentions: BTreeSet<u64>,
	pub connections: Vec<Value>,
	pub guild_bios: HashMap<(u64, u64), String>,
	pub audit: HashMap<u64, Vec<Value>>,
	pub joined_threads: BTreeSet<u64>,
	pub thread_member_settings: HashMap<u64, Value>,
	pub thread_invitees: HashMap<u64, BTreeSet<u64>>,
	pub slowmode: HashMap<u64, Instant>,
	pub pinned_at: HashMap<u64, DateTime<Utc>>,
	pub frecency: ProtoFrecencySettings,
	pub proto_status: Vec<u8>,
	pub proto_privacy: Vec<u8>,
	pub proto_text_and_images: Vec<u8>,
	pub uploads: HashMap<String, u64>,
	pub nonces: HashMap<(u64, String), u64>,
	pub stage_instances: HashMap<u64, StageInstance>,
	pub seats: HashMap<u64, Seat>,
	pub sounds: HashMap<u64, Vec<SoundboardSound>>,
	pub calls: BTreeMap<u64, Vec<u64>>,
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
			scheduled_events: fixtures
				.guilds
				.iter()
				.map(|guild| (guild.id, guild.scheduled_events.clone()))
				.collect(),
			event_interest: BTreeSet::new(),
			removed_members: HashMap::new(),
			bans: crate::admin::seed_bans(fixtures),
			automod_rules: crate::automod::seed_automod(fixtures),
			member_edits: HashMap::new(),
			invites: BTreeMap::new(),
			absent_guilds: fixtures
				.guilds
				.iter()
				.filter(|guild| guild.origin != GuildOrigin::Seeded)
				.map(|guild| guild.id)
				.collect(),
			ended_sessions: BTreeSet::new(),
			dismissed_mentions: BTreeSet::new(),
			connections: seed_connections(),
			guild_bios: HashMap::new(),
			audit: crate::admin::seed_audit(fixtures),
			pinned_at: fixtures.pinned_at(),
			joined_threads: fixtures
				.guilds
				.first()
				.and_then(|guild| guild.thread_ids.first())
				.into_iter()
				.chain(fixtures.guilds.iter().flat_map(|guild| &guild.private_thread_ids))
				.copied()
				.collect(),
			thread_member_settings: HashMap::new(),
			thread_invitees: HashMap::new(),
			slowmode: HashMap::new(),
			proto_status: seed_status(),
			proto_privacy: seed_privacy(fixtures),
			proto_text_and_images: encode_sensitive_media(&MOCK_SENSITIVE_MEDIA),
			frecency: ProtoFrecencySettings {
				data_version: 1,
				..ProtoFrecencySettings::default()
			},
			uploads: HashMap::new(),
			nonces: HashMap::new(),
			stage_instances: fixtures
				.guilds
				.iter()
				.filter_map(|guild| guild.stage_instance.as_ref())
				.map(|instance| (instance.channel_id.0, instance.clone()))
				.collect(),
			seats: HashMap::new(),
			sounds: crate::soundboard::seed_sounds(fixtures),
			calls: BTreeMap::new(),
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
	pub stream_previews: Mutex<HashSet<String>>,
	pub stream_unseen: Mutex<HashSet<String>>,
	pub send_fault: Mutex<Option<SendFault>>,
	pub slow_uploads: AtomicBool,
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
			stream_previews: Mutex::new(HashSet::new()),
			stream_unseen: Mutex::new(HashSet::new()),
			send_fault: Mutex::new(None),
			slow_uploads: AtomicBool::new(false),
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
