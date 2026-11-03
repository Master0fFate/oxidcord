use std::collections::BTreeSet;

use protocol::{Channel, Member, Permissions, Presence, StatusKind};
use serde_json::{Map, Value, json};

use crate::fixtures::GuildFixture;
use crate::json::{parse_id, to_json};
use crate::state::Shared;

type Range = (usize, usize);

const EVERYONE_LIST: &str = "everyone";
const RESTRICTED_LIST: &str = "restricted";

fn display_name(member: &Member) -> String {
	member
		.nick
		.clone()
		.or_else(|| {
			member
				.user
				.as_ref()
				.map(|user| user.global_name.clone().unwrap_or_else(|| user.username.clone()))
		})
		.unwrap_or_default()
		.to_lowercase()
}

fn member_user_id(member: &Member) -> u64 {
	member.user.as_ref().map(|user| user.id.0).unwrap_or_default()
}

pub fn presence_json(guild_id: u64, user_id: u64, presence: &Presence) -> Value {
	let mut value = to_json(presence);
	value["user"] = json!({ "id": user_id.to_string() });
	value["guild_id"] = json!(guild_id.to_string());
	value
}

fn offline_json(guild_id: u64, user_id: u64) -> Value {
	json!({
		"user": { "id": user_id.to_string() },
		"guild_id": guild_id.to_string(),
		"status": "offline",
		"activities": [],
	})
}

fn member_item(guild: &GuildFixture, member: &Member, presence: Option<&Presence>) -> Value {
	let user_id = member_user_id(member);
	let mut value = to_json(member);
	value["presence"] = presence.map_or_else(
		|| offline_json(guild.id, user_id),
		|presence| presence_json(guild.id, user_id, presence),
	);
	json!({ "member": value })
}

struct MemberList {
	items: Vec<Value>,
	online: usize,
	offline: usize,
}

fn is_restricted(guild: &GuildFixture, channel: &Channel) -> bool {
	channel
		.permission_overwrites
		.iter()
		.any(|overwrite| overwrite.id.0 == guild.id && overwrite.deny.contains(Permissions::VIEW_CHANNEL))
}

fn can_view(channel: &Channel, member: &Member) -> bool {
	channel.permission_overwrites.iter().any(|overwrite| {
		overwrite.allow.contains(Permissions::VIEW_CHANNEL) && member.roles.contains(&overwrite.id)
	})
}

fn build_member_list(shared: &Shared, guild: &GuildFixture, channel: &Channel) -> MemberList {
	let restricted = is_restricted(guild, channel);
	let store = shared.store.lock();
	let presences = store.presences.get(&guild.id);
	let mut online: Vec<(&Member, &Presence)> = Vec::new();
	let mut offline: Vec<&Member> = Vec::new();
	for member in guild
		.members
		.iter()
		.filter(|member| !store.is_removed(guild.id, member_user_id(member)))
		.filter(|member| !restricted || can_view(channel, member))
	{
		match presences
			.and_then(|by_user| by_user.get(&member_user_id(member)))
			.filter(|presence| presence.status != StatusKind::Offline)
		{
			Some(presence) => online.push((member, presence)),
			None => offline.push(member),
		}
	}
	online.sort_by_cached_key(|(member, _)| display_name(member));
	offline.sort_by_cached_key(|member| display_name(member));
	let mut items = Vec::with_capacity(guild.members.len() + 2);
	items.push(json!({ "group": { "id": "online", "count": online.len() } }));
	items.extend(
		online
			.iter()
			.map(|(member, presence)| member_item(guild, member, Some(presence))),
	);
	items.push(json!({ "group": { "id": "offline", "count": offline.len() } }));
	items.extend(offline.iter().map(|member| member_item(guild, member, None)));
	MemberList {
		items,
		online: online.len(),
		offline: offline.len(),
	}
}

fn update_for(shared: &Shared, guild_id: u64, channel_id: u64, ranges: &[Range]) -> Option<Value> {
	if ranges.is_empty() {
		return None;
	}
	let guild = shared.fixtures.guild(guild_id)?;
	let channel = shared.fixtures.channels.get(&channel_id)?;
	let list = build_member_list(shared, guild, channel);
	let ops: Vec<Value> = ranges
		.iter()
		.map(|&(start, end)| {
			let stop = (end + 1).min(list.items.len());
			let begin = start.min(stop);
			json!({ "op": "SYNC", "range": [start, end], "items": &list.items[begin..stop] })
		})
		.collect();
	Some(json!({
		"guild_id": guild_id.to_string(),
		"id": if is_restricted(guild, channel) { RESTRICTED_LIST } else { EVERYONE_LIST },
		"member_count": list.online + list.offline,
		"online_count": list.online,
		"groups": [
			{ "id": "online", "count": list.online },
			{ "id": "offline", "count": list.offline },
		],
		"ops": ops,
	}))
}

fn ranges_of(ranges: &Value) -> Vec<Range> {
	let unique: BTreeSet<Range> = ranges
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|range| {
			let bounds = range.as_array()?;
			Some((
				bounds.first()?.as_u64()? as usize,
				bounds.get(1)?.as_u64()? as usize,
			))
		})
		.collect();
	unique.into_iter().collect()
}

fn channel_updates(shared: &Shared, guild_id: u64, channels: &Value) -> Vec<Value> {
	let empty = Map::new();
	channels
		.as_object()
		.unwrap_or(&empty)
		.iter()
		.filter_map(|(channel_key, ranges)| {
			update_for(shared, guild_id, channel_key.parse().ok()?, &ranges_of(ranges))
		})
		.collect()
}

pub fn bulk_guild_ids(data: &Value) -> impl Iterator<Item = u64> + '_ {
	data["subscriptions"]
		.as_object()
		.into_iter()
		.flatten()
		.filter_map(|(guild_key, _)| guild_key.parse().ok())
}

pub fn bulk_updates(shared: &Shared, data: &Value) -> Vec<Value> {
	let empty = Map::new();
	data["subscriptions"]
		.as_object()
		.unwrap_or(&empty)
		.iter()
		.filter_map(|(guild_key, subscription)| {
			Some(channel_updates(
				shared,
				guild_key.parse().ok()?,
				&subscription["channels"],
			))
		})
		.flatten()
		.collect()
}

pub fn legacy_updates(shared: &Shared, data: &Value) -> Vec<Value> {
	parse_id(&data["guild_id"])
		.map(|guild_id| channel_updates(shared, guild_id, &data["channels"]))
		.unwrap_or_default()
}

pub fn members_chunks(shared: &Shared, data: &Value) -> Vec<Value> {
	let guild_ids: Vec<u64> = match &data["guild_id"] {
		Value::Array(ids) => ids.iter().filter_map(parse_id).collect(),
		single => parse_id(single).into_iter().collect(),
	};
	let query = data["query"].as_str().unwrap_or("").to_lowercase();
	let limit = data["limit"].as_u64().unwrap_or(0) as usize;
	let wanted: Vec<u64> = data["user_ids"]
		.as_array()
		.map(|ids| ids.iter().filter_map(parse_id).collect())
		.unwrap_or_default();
	let with_presences = data["presences"].as_bool() == Some(true);
	let store = shared.store.lock();
	guild_ids
		.into_iter()
		.filter_map(|guild_id| {
			let guild = shared.fixtures.guild(guild_id)?;
			let selected: Vec<&Member> = if wanted.is_empty() {
				let matching = guild
					.members
					.iter()
					.filter(|member| !store.is_removed(guild_id, member_user_id(member)))
					.filter(|member| {
						let user = member.user.as_ref();
						display_name(member).starts_with(&query)
							|| user.is_some_and(|user| user.username.to_lowercase().starts_with(&query))
					});
				if limit == 0 {
					matching.collect()
				} else {
					matching.take(limit).collect()
				}
			} else {
				wanted.iter().filter_map(|id| guild.member(*id)).collect()
			};
			let not_found: Vec<String> = wanted
				.iter()
				.filter(|id| guild.member(**id).is_none())
				.map(u64::to_string)
				.collect();
			let presences: Vec<Value> = if with_presences {
				selected
					.iter()
					.filter_map(|member| {
						let user_id = member_user_id(member);
						let presence = store.presences.get(&guild_id)?.get(&user_id)?;
						Some(presence_json(guild_id, user_id, presence))
					})
					.collect()
			} else {
				Vec::new()
			};
			Some(json!({
				"guild_id": guild_id.to_string(),
				"members": selected,
				"chunk_index": 0,
				"chunk_count": 1,
				"not_found": not_found,
				"presences": presences,
				"nonce": data["nonce"],
			}))
		})
		.collect()
}
