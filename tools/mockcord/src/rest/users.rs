use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use protocol::{Channel, ChannelType, LenientVec, Snowflake, User};
use serde_json::{Value, json};

use super::admin;
use crate::error::{ApiError, ApiResult};
use crate::fixtures::CURRENT_USER_ID;
use crate::json::{parse_id, to_json};
use crate::state::Shared;

const STAGE_INVITE: &str = "mockstage";
const EVENT_INVITE: &str = "mockevent";

pub async fn gateway_url(State(shared): State<Arc<Shared>>) -> Json<Value> {
	Json(json!({ "url": shared.urls.gateway }))
}

pub async fn me(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> ApiResult {
	headers
		.get(AUTHORIZATION)
		.and_then(|value| value.to_str().ok())
		.and_then(|token| shared.account_for_token(token))
		.map(|user| Json(to_json(&user)))
		.ok_or_else(ApiError::unauthorized)
}

fn mentions_me(message: &Value, include_everyone: bool, my_roles: Option<&[String]>) -> bool {
	let me = CURRENT_USER_ID.to_string();
	let direct = message["mentions"]
		.as_array()
		.is_some_and(|users| users.iter().any(|user| user["id"].as_str() == Some(&me)));
	let by_role = my_roles.is_some_and(|roles| {
		message["mention_roles"].as_array().is_some_and(|mentioned| {
			mentioned
				.iter()
				.any(|role| roles.iter().any(|mine| role.as_str() == Some(mine)))
		})
	});
	direct || by_role || (include_everyone && message["mention_everyone"].as_bool() == Some(true))
}

pub async fn my_mentions(
	State(shared): State<Arc<Shared>>,
	Query(params): Query<HashMap<String, String>>,
) -> Json<Value> {
	let limit = params
		.get("limit")
		.and_then(|limit| limit.parse().ok())
		.unwrap_or(25)
		.min(100);
	let include_everyone = params.get("everyone").is_none_or(|value| value != "false");
	let my_roles: Vec<String> = shared
		.fixtures
		.guilds
		.iter()
		.flat_map(|guild| guild.current_member().roles.iter().map(ToString::to_string))
		.collect();
	let include_roles = params.get("roles").is_none_or(|value| value != "false");
	let role_filter = include_roles.then_some(my_roles.as_slice());
	let store = shared.store.lock();
	let mut found: Vec<(&u64, &Value)> = store
		.messages
		.values()
		.flatten()
		.filter(|(id, message)| {
			!store.dismissed_mentions.contains(id)
				&& message["author"]["id"].as_str() != Some(&CURRENT_USER_ID.to_string())
				&& mentions_me(message, include_everyone, role_filter)
		})
		.collect();
	found.sort_unstable_by_key(|(id, _)| std::cmp::Reverse(**id));
	Json(Value::Array(
		found
			.into_iter()
			.take(limit)
			.map(|(_, message)| message.clone())
			.collect(),
	))
}

pub async fn my_connections(State(shared): State<Arc<Shared>>) -> Json<Value> {
	Json(Value::Array(shared.store.lock().connections.clone()))
}

pub async fn my_guilds(State(shared): State<Arc<Shared>>) -> Json<Value> {
	let store = shared.store.lock();
	let guilds: Vec<Value> = shared
		.fixtures
		.guilds
		.iter()
		.filter(|guild| !store.absent_guilds.contains(&guild.id))
		.map(|guild| {
			json!({
				"id": guild.id.to_string(),
				"name": store.properties[&guild.id].name,
				"icon": store.properties[&guild.id].icon,
				"banner": null,
				"owner": guild.properties.owner_id.map(Snowflake::get) == Some(CURRENT_USER_ID),
				"permissions": guild.permissions_for(guild.current_member()).bits().to_string(),
				"features": guild.properties.features,
				"approximate_member_count": store.live_member_count(guild),
				"approximate_presence_count": store.online_count(guild.id),
			})
		})
		.collect();
	Json(Value::Array(guilds))
}

pub async fn relationships(State(shared): State<Arc<Shared>>) -> Json<Value> {
	Json(Value::Array(
		shared.store.lock().relationships.values().map(to_json).collect(),
	))
}

pub async fn user(State(shared): State<Arc<Shared>>, Path(id): Path<u64>) -> ApiResult {
	shared
		.fixtures
		.users
		.get(&id)
		.map(|user| Json(to_json(user)))
		.ok_or_else(ApiError::unknown_user)
}

pub async fn profile(
	State(shared): State<Arc<Shared>>,
	Path(id): Path<u64>,
	Query(params): Query<HashMap<String, String>>,
) -> ApiResult {
	shared.simulate_latency().await;
	let fixtures = &shared.fixtures;
	let user = fixtures.users.get(&id).ok_or_else(ApiError::unknown_user)?;
	let mutual_guilds: Vec<Value> = fixtures
		.guilds
		.iter()
		.filter_map(|guild| {
			let member = guild.member(id)?;
			Some(json!({ "id": guild.id.to_string(), "nick": member.nick }))
		})
		.collect();
	let extras = admin::profile_extras(
		&shared,
		id,
		params.get("guild_id").and_then(|guild| guild.parse().ok()),
	);
	let mut meta = json!({
		"bio": format!("hi, i am {}", user.username),
		"accent_color": null,
		"pronouns": "",
		"banner": null,
		"theme_colors": extras["theme_colors"],
		"profile_effect": extras["profile_effect"],
	});
	if id == CURRENT_USER_ID {
		for (key, value) in shared.store.lock().profile.iter() {
			meta[key] = value.clone();
		}
	}
	let friends = extras["mutual_friends"].as_array().map_or(0, Vec::len);
	Ok(Json(json!({
		"user": user,
		"user_profile": meta,
		"mutual_guilds": mutual_guilds,
		"mutual_friends": extras["mutual_friends"],
		"mutual_friends_count": friends,
		"connected_accounts": extras["connected_accounts"],
		"guild_member": extras["guild_member"],
		"guild_member_profile": extras["guild_member_profile"],
		"premium_since": null,
		"premium_type": null,
		"badges": extras["badges"],
		"guild_badges": extras["guild_badges"],
		"legacy_username": extras["legacy_username"],
		"private": extras["private"],
	})))
}

pub async fn create_dm(State(shared): State<Arc<Shared>>, Json(body): Json<Value>) -> ApiResult {
	let mut recipient_ids: Vec<u64> = body["recipients"]
		.as_array()
		.map(|ids| ids.iter().filter_map(parse_id).collect())
		.unwrap_or_default();
	recipient_ids.extend(parse_id(&body["recipient_id"]));
	if recipient_ids.is_empty() {
		return Err(ApiError::invalid_body("recipients is required"));
	}
	let recipients: Vec<User> = recipient_ids
		.iter()
		.map(|id| {
			shared
				.fixtures
				.users
				.get(id)
				.cloned()
				.ok_or_else(ApiError::unknown_user)
		})
		.collect::<Result<_, _>>()?;
	let kind = if recipients.len() > 1 {
		ChannelType::GroupDm
	} else {
		ChannelType::Dm
	};
	let mut wanted: Vec<Snowflake> = recipients.iter().map(|user| user.id).collect();
	wanted.sort();
	let mut store = shared.store.lock();
	let existing = store
		.private_channel_ids
		.iter()
		.filter_map(|id| store.channels.get(id))
		.find(|channel| {
			let mut have = channel.recipient_ids.clone();
			have.sort();
			channel.kind == kind && have == wanted
		});
	if let Some(channel) = existing {
		return Ok(Json(to_json(channel)));
	}
	let id = store.next_snowflake();
	let channel = Channel {
		id: Snowflake(id),
		kind,
		owner_id: (kind == ChannelType::GroupDm).then_some(Snowflake(CURRENT_USER_ID)),
		recipient_ids: wanted,
		recipients: LenientVec::from(recipients),
		..Channel::default()
	};
	let value = to_json(&channel);
	store.channels.insert(id, channel);
	store.private_channel_ids.push(id);
	store.messages.entry(id).or_default();
	drop(store);
	shared.hub.broadcast("CHANNEL_CREATE", &value);
	Ok(Json(value))
}

pub async fn invite(
	State(shared): State<Arc<Shared>>,
	Path(code): Path<String>,
	Query(params): Query<HashMap<String, String>>,
) -> ApiResult {
	if let Some(application) = shared.application_invite(&code, false) {
		return Ok(Json(application));
	}
	if let Some(created) = shared.invite_json(&code) {
		return Ok(Json(created));
	}
	let on_stage = code == STAGE_INVITE;
	let lookup = if on_stage || code == EVENT_INVITE {
		"mock"
	} else {
		code.as_str()
	};
	let guild = shared
		.fixture_invite_guild(lookup)
		.ok_or_else(ApiError::unknown_invite)?;
	let properties = &guild.properties;
	let channel_id = if on_stage {
		guild
			.stage_instance
			.as_ref()
			.map(|instance| instance.channel_id.0)
	} else {
		guild.message_channel_ids.first().copied()
	};
	let channel = channel_id.and_then(|id| shared.fixtures.channels.get(&id));
	let online = shared.store.lock().online_count(guild.id);
	let event = params
		.get("guild_scheduled_event_id")
		.and_then(|id| id.parse::<u64>().ok())
		.and_then(|id| {
			if code == EVENT_INVITE {
				shared.first_open_event_json(guild.id)
			} else {
				shared.invite_event_json(guild.id, id)
			}
		});
	Ok(Json(json!({
		"type": 0,
		"code": code,
		"expires_at": null,
		"guild_id": guild.id.to_string(),
		"guild": {
			"id": guild.id.to_string(),
			"name": properties.name,
			"icon": properties.icon,
			"splash": null,
			"banner": null,
			"description": properties.description,
			"features": properties.features,
			"verification_level": properties.verification_level,
			"vanity_url_code": null,
			"nsfw_level": properties.nsfw_level,
			"premium_subscription_count": properties.premium_subscription_count,
		},
		"channel": channel.map(|channel| json!({
			"id": channel.id.to_string(),
			"type": channel.kind,
			"name": channel.name,
		})),
		"inviter": shared.fixtures.users.get(&properties.owner_id.map_or(0, Snowflake::get)),
		"approximate_member_count": guild.members.len(),
		"approximate_presence_count": online,
		"guild_scheduled_event": event,
	})))
}
