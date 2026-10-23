use std::collections::BTreeMap;

use protocol::{Channel, ChannelType, Presence, RelationshipKind, Snowflake, User};
use serde_json::{Value, json};

use crate::fixtures::{Fixtures, GuildFixture};
use crate::json::to_json;
use crate::state::{Shared, Store};

const API_VERSION: i64 = 10;
const CONTAINER_VERSION: i64 = 1;
const MOCK_FOLDER_ID: i64 = 1_001;
const MOCK_FOLDER_COLOR: i64 = 0x6666ff;

pub fn is_thread(kind: ChannelType) -> bool {
	matches!(
		kind,
		ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::AnnouncementThread
	)
}

pub fn guild_threads(store: &Store, guild_id: u64) -> Vec<&Channel> {
	let mut threads: Vec<&Channel> = store
		.channels
		.values()
		.filter(|channel| channel.guild_id == Some(Snowflake(guild_id)) && is_thread(channel.kind))
		.collect();
	threads.sort_by_key(|channel| channel.id);
	threads
}

fn ready_guild(store: &Store, guild: &GuildFixture) -> Value {
	let threads: Vec<Value> = guild_threads(store, guild.id)
		.into_iter()
		.map(|thread| store.thread_json(thread))
		.collect();
	json!({
		"id": guild.id.to_string(),
		"properties": store.properties[&guild.id],
		"channels": store.guild_channels(guild),
		"roles": store.roles[&guild.id],
		"emojis": store.emojis[&guild.id],
		"stickers": [],
		"threads": threads,
		"members": [],
		"voice_states": guild.voice_states,
		"presences": [],
		"member_count": store.live_member_count(guild),
		"joined_at": guild.joined_at,
		"large": false,
		"premium_subscription_count": guild.properties.premium_subscription_count,
		"stage_instances": [],
		"guild_scheduled_events": [],
	})
}

fn live_guilds<'a>(fixtures: &'a Fixtures, store: &Store) -> impl Iterator<Item = &'a GuildFixture> {
	fixtures
		.guilds
		.iter()
		.filter(|guild| !store.left_guilds.contains(&guild.id))
}

fn merged_members(fixtures: &Fixtures, store: &Store) -> Value {
	let per_guild: Vec<Vec<Value>> = live_guilds(fixtures, store)
		.map(|guild| vec![to_json(guild.current_member())])
		.collect();
	json!(per_guild)
}

fn referenced_users<'a>(fixtures: &'a Fixtures, store: &Store) -> Vec<&'a User> {
	let mut users: BTreeMap<u64, &User> = BTreeMap::new();
	for relationship in store.relationships.values() {
		if let Some(user) = fixtures.users.get(&relationship.id.0) {
			users.insert(relationship.id.0, user);
		}
	}
	for channel_id in &store.private_channel_ids {
		let recipient_ids = store
			.channels
			.get(channel_id)
			.map(|channel| channel.recipient_ids.as_slice());
		for id in recipient_ids.unwrap_or_default() {
			if let Some(user) = fixtures.users.get(&id.0) {
				users.insert(id.0, user);
			}
		}
	}
	for guild in live_guilds(fixtures, store) {
		for state in &guild.voice_states {
			if let Some(user) = fixtures.users.get(&state.user_id.0) {
				users.insert(state.user_id.0, user);
			}
		}
	}
	users.into_values().collect()
}

fn guild_presences(store: &Store, guild_id: u64) -> Vec<&Presence> {
	let mut presences: Vec<&Presence> = store
		.presences
		.get(&guild_id)
		.map(|by_user| by_user.values().collect())
		.unwrap_or_default();
	presences.sort_by_key(|presence| presence.user_id);
	presences
}

fn friend_presences<'a>(fixtures: &Fixtures, store: &'a Store) -> Vec<&'a Presence> {
	let mut seen: BTreeMap<u64, &Presence> = BTreeMap::new();
	for guild in live_guilds(fixtures, store) {
		for presence in guild_presences(store, guild.id) {
			if let Some(user_id) = presence.user_id {
				seen.entry(user_id.0).or_insert(presence);
			}
		}
	}
	store
		.relationships
		.values()
		.filter(|relationship| relationship.kind == RelationshipKind::Friend)
		.filter_map(|relationship| seen.get(&relationship.id.0).copied())
		.collect()
}

pub fn build(shared: &Shared, session_id: &str) -> (Value, Value) {
	let fixtures = &shared.fixtures;
	let store = shared.store.lock();
	let guilds: Vec<Value> = live_guilds(fixtures, &store)
		.map(|guild| ready_guild(&store, guild))
		.collect();
	let folder_guild_ids: Vec<String> = live_guilds(fixtures, &store)
		.take(2)
		.map(|guild| guild.id.to_string())
		.collect();
	let private_channels: Vec<&Channel> = store
		.private_channel_ids
		.iter()
		.filter_map(|id| store.channels.get(id))
		.collect();
	let ready = json!({
		"v": API_VERSION,
		"user": store.current_user,
		"guilds": guilds,
		"private_channels": private_channels,
		"relationships": store.relationships.values().collect::<Vec<_>>(),
		"read_state": {
			"entries": store.read_states.values().collect::<Vec<_>>(),
			"version": CONTAINER_VERSION,
			"partial": false,
		},
		"user_guild_settings": {
			"entries": store.settings.values().collect::<Vec<_>>(),
			"version": CONTAINER_VERSION,
			"partial": false,
		},
		"session_id": session_id,
		"resume_gateway_url": shared.urls.gateway,
		"sessions": [{
			"session_id": session_id,
			"status": "online",
			"activities": [],
			"client_info": { "client": "desktop", "os": "windows", "version": 0 },
			"active": true,
		}],
		"users": referenced_users(fixtures, &store),
		"merged_members": merged_members(fixtures, &store),
		"analytics_token": "x",
		"user_settings_proto": null,
		"user_settings": {
			"guild_folders": [{
				"id": MOCK_FOLDER_ID,
				"name": "Mock folder",
				"color": MOCK_FOLDER_COLOR,
				"guild_ids": folder_guild_ids,
			}],
		},
	});
	let supplemental = json!({
		"merged_presences": {
			"guilds": live_guilds(fixtures, &store)
				.map(|guild| guild_presences(&store, guild.id))
				.collect::<Vec<_>>(),
			"friends": friend_presences(fixtures, &store),
		},
		"merged_members": merged_members(fixtures, &store),
		"guilds": live_guilds(fixtures, &store)
			.map(|guild| json!({ "id": guild.id.to_string(), "voice_states": guild.voice_states }))
			.collect::<Vec<_>>(),
	});
	(ready, supplemental)
}
