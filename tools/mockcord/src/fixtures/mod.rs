pub(crate) mod corpus;
mod guilds;
mod media;
mod messages;
mod people;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use protocol::{
	Channel, ChannelOverride, LenientVec, MuteConfig, NotificationLevel, ReadState, ReadStateKind,
	Relationship, Snowflake, User, UserGuildSettings,
};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::Serialize;
use serde_json::Value;

pub use guilds::{GuildFixture, random_presence};

use crate::config::MockConfig;
use crate::ids::EntityIds;
use crate::json::mentions_user;

pub const CURRENT_USER_ID: u64 = 100;
const MIN_POOL_SIZE: usize = 40;
const READ_CUTOFF_PERCENT: usize = 85;
const SETTINGS_VERSION: i64 = 1;

struct Builder {
	rng: StdRng,
	ids: EntityIds,
	config: MockConfig,
	anchor: DateTime<Utc>,
	people: people::People,
	channels: BTreeMap<u64, Channel>,
	messages: BTreeMap<u64, BTreeMap<u64, Value>>,
}

pub struct Fixtures {
	pub anchor: DateTime<Utc>,
	pub current_user: User,
	pub users: BTreeMap<u64, User>,
	pub guilds: Vec<GuildFixture>,
	pub channels: BTreeMap<u64, Channel>,
	pub private_channel_ids: Vec<u64>,
	pub relationships: Vec<Relationship>,
	pub read_states: Vec<ReadState>,
	pub user_guild_settings: Vec<UserGuildSettings>,
	pub messages: BTreeMap<u64, BTreeMap<u64, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixtureSummary {
	pub guilds: usize,
	pub channels: usize,
	pub messages: usize,
	pub members: usize,
	pub unread_channels: usize,
	pub total_mentions: i64,
}

impl Fixtures {
	pub fn generate(config: &MockConfig, anchor: DateTime<Utc>) -> Fixtures {
		let mut rng = StdRng::seed_from_u64(config.seed);
		let mut ids = EntityIds::default();
		let pool_size = (config.members_per_guild * 2).max(MIN_POOL_SIZE);
		let people = people::generate(&mut rng, &mut ids, pool_size);
		let mut builder = Builder {
			rng,
			ids,
			config: config.clone(),
			anchor,
			people,
			channels: BTreeMap::new(),
			messages: BTreeMap::new(),
		};
		let mut guilds = Vec::with_capacity(config.guilds);
		for index in 0..config.guilds {
			guilds.push(guilds::build(&mut builder, index));
		}
		let private_channel_ids = people::build_private_channels(&mut builder);
		let relationships = people::build_relationships(&builder);
		let read_states = build_read_states(&builder, &guilds, &private_channel_ids);
		let user_guild_settings = build_user_guild_settings(&guilds);
		let mut users: BTreeMap<u64, User> = builder
			.people
			.pool
			.iter()
			.map(|user| (user.id.0, user.clone()))
			.collect();
		users.insert(CURRENT_USER_ID, builder.people.current.clone());
		Fixtures {
			anchor,
			current_user: builder.people.current,
			users,
			guilds,
			channels: builder.channels,
			private_channel_ids,
			relationships,
			read_states,
			user_guild_settings,
			messages: builder.messages,
		}
	}

	pub fn guild(&self, id: u64) -> Option<&GuildFixture> {
		self.guilds.iter().find(|guild| guild.id == id)
	}

	pub fn summary(&self) -> FixtureSummary {
		let unread_channels = self
			.read_states
			.iter()
			.filter(|state| {
				let last = self
					.channels
					.get(&state.id.0)
					.and_then(|channel| channel.last_message_id);
				matches!((state.last_message_id, last), (Some(read), Some(last)) if read < last)
			})
			.count();
		FixtureSummary {
			guilds: self.guilds.len(),
			channels: self.channels.len(),
			messages: self.messages.values().map(BTreeMap::len).sum(),
			members: self.guilds.iter().map(|guild| guild.members.len()).sum(),
			unread_channels,
			total_mentions: self
				.read_states
				.iter()
				.filter_map(|state| state.mention_count)
				.sum(),
		}
	}
}

fn build_read_states(builder: &Builder, guilds: &[GuildFixture], private: &[u64]) -> Vec<ReadState> {
	let channel_ids = guilds
		.iter()
		.flat_map(|guild| guild.message_channel_ids.iter().copied())
		.chain(private.iter().copied());
	channel_ids
		.enumerate()
		.filter_map(|(position, channel_id)| {
			let messages = builder
				.messages
				.get(&channel_id)
				.filter(|messages| !messages.is_empty())?;
			Some(read_state(channel_id, messages, position % 3 != 2))
		})
		.collect()
}

fn read_state(channel_id: u64, messages: &BTreeMap<u64, Value>, unread: bool) -> ReadState {
	let ids: Vec<u64> = messages.keys().copied().collect();
	let newest = ids[ids.len() - 1];
	let (last_read, mention_count) = if unread && ids.len() > 1 {
		let cutoff = (ids.len() * READ_CUTOFF_PERCENT / 100).min(ids.len() - 2);
		let last_read = ids[cutoff];
		let mentions = messages
			.range(last_read + 1..)
			.filter(|(_, message)| mentions_user(message, CURRENT_USER_ID))
			.count();
		(last_read, mentions as i64)
	} else {
		(newest, 0)
	};
	ReadState {
		id: Snowflake(channel_id),
		last_message_id: Some(Snowflake(last_read)),
		last_pin_timestamp: None,
		mention_count: Some(mention_count),
		flags: Some(0),
		last_viewed: Some(4000),
		read_state_type: Some(ReadStateKind::Channel),
	}
}

fn build_user_guild_settings(guilds: &[GuildFixture]) -> Vec<UserGuildSettings> {
	guilds
		.iter()
		.enumerate()
		.map(|(index, guild)| {
			let muted = index == 1;
			let forever = || MuteConfig {
				end_time: None,
				selected_time_window: Some(-1),
			};
			let channel_overrides = if index == 0 {
				vec![ChannelOverride {
					channel_id: Snowflake(guild.muted_channel_id),
					muted: true,
					mute_config: Some(forever()),
					message_notifications: Some(NotificationLevel::Inherit),
					collapsed: Some(false),
					flags: Some(0),
				}]
			} else {
				Vec::new()
			};
			UserGuildSettings {
				guild_id: Some(Snowflake(guild.id)),
				muted,
				mute_config: muted.then(forever),
				message_notifications: Some(if index == 2 {
					NotificationLevel::OnlyMentions
				} else {
					NotificationLevel::AllMessages
				}),
				suppress_everyone: false,
				suppress_roles: false,
				hide_muted_channels: false,
				channel_overrides: LenientVec::from(channel_overrides),
				notify_highlights: Some(0),
				flags: Some(0),
				version: Some(SETTINGS_VERSION),
			}
		})
		.collect()
}
