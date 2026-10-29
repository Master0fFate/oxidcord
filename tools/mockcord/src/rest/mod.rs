mod admin;
mod apps;
mod channels;
mod expressions;
mod guilds;
mod rpc;
mod users;

pub use rpc::{RPC_APP_ID, RPC_APP_NAME, RPC_ASSET_ID, RPC_ASSET_NAME};

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, MatchedPath, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use chrono::Utc;
use protocol::RateLimitBody;
use rand::Rng;

use crate::error::ApiError;
use crate::gateway;
use crate::media;
use crate::state::{RateLimitSnapshot, Shared};

const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
const BUCKET_LIMIT: u32 = 5;
const CHAOS_RATE_LIMIT_PERCENT: f64 = 0.05;
const CHAOS_RETRY_AFTER: f64 = 0.5;

pub fn router(shared: Arc<Shared>) -> Router {
	let api = Router::new()
		.route("/gateway", get(users::gateway_url))
		.route("/users/@me", get(users::me).patch(admin::patch_me))
		.route("/users/@me/profile", patch(admin::patch_profile))
		.route("/users/@me/settings", patch(admin::patch_settings))
		.route("/users/@me/guilds", get(users::my_guilds))
		.route("/users/@me/mentions", get(users::my_mentions))
		.route(
			"/users/@me/saved-messages/{id}/{mid}",
			put(channels::save_message).delete(channels::save_message),
		)
		.route("/guilds", post(admin::create_guild))
		.route("/auth/sessions", get(admin::auth_sessions))
		.route("/auth/sessions/logout", post(admin::logout_sessions))
		.route("/users/@me/guilds/{id}", delete(admin::leave_guild))
		.route(
			"/users/@me/guilds/{id}/settings",
			patch(admin::patch_guild_settings),
		)
		.route(
			"/users/@me/relationships",
			get(users::relationships).post(admin::add_friend),
		)
		.route(
			"/users/@me/relationships/{id}",
			put(admin::put_relationship).delete(admin::delete_relationship),
		)
		.route("/users/@me/channels", post(users::create_dm))
		.route("/users/{id}", get(users::user))
		.route("/users/{id}/profile", get(users::profile))
		.route("/read-states/ack-bulk", post(admin::ack_bulk))
		.route(
			"/invites/{code}",
			get(users::invite)
				.post(admin::accept_invite)
				.delete(admin::delete_invite),
		)
		.route("/guilds/{id}", get(guilds::guild).patch(admin::modify_guild))
		.route(
			"/guilds/{id}/channels",
			get(guilds::channels).post(admin::create_channel),
		)
		.route(
			"/guilds/{id}/roles",
			get(guilds::roles)
				.post(admin::create_role)
				.patch(admin::reorder_roles),
		)
		.route(
			"/guilds/{id}/roles/{rid}",
			patch(admin::modify_role).delete(admin::delete_role),
		)
		.route(
			"/guilds/{id}/emojis",
			get(guilds::emojis).post(admin::create_emoji),
		)
		.route("/guilds/{id}/emojis/{eid}", delete(admin::delete_emoji))
		.route("/guilds/{id}/members/search", get(guilds::search_members))
		.route("/guilds/{id}/members/@me", patch(admin::modify_current_member))
		.route(
			"/guilds/{id}/members/{uid}",
			get(guilds::member)
				.patch(admin::modify_member)
				.delete(admin::kick),
		)
		.route(
			"/guilds/{id}/members/{uid}/roles/{rid}",
			put(admin::add_member_role).delete(admin::remove_member_role),
		)
		.route("/guilds/{id}/bans", get(admin::bans))
		.route("/guilds/{id}/bans/{uid}", put(admin::ban).delete(admin::unban))
		.route("/guilds/{id}/invites", get(admin::guild_invites))
		.route("/guilds/{id}/audit-logs", get(admin::audit_log))
		.route("/guilds/{id}/threads/active", get(admin::active_threads))
		.route("/guilds/{id}/messages/search", get(guilds::search_messages))
		.route(
			"/channels/{id}",
			get(channels::channel)
				.patch(admin::modify_channel)
				.delete(admin::delete_channel),
		)
		.route(
			"/channels/{id}/messages",
			get(channels::list_messages).post(channels::create_message),
		)
		.route("/channels/{id}/messages/search", get(channels::search_messages))
		.route("/channels/{id}/messages/bulk-delete", post(admin::bulk_delete))
		.route("/channels/{id}/messages/{mid}/crosspost", post(channels::message))
		.route(
			"/channels/{id}/messages/{mid}",
			get(channels::message)
				.patch(channels::edit_message)
				.delete(channels::delete_message),
		)
		.route(
			"/channels/{id}/messages/{mid}/reactions/{emoji}",
			get(admin::reactions),
		)
		.route(
			"/channels/{id}/messages/{mid}/reactions/{emoji}/@me",
			put(channels::add_reaction).delete(channels::remove_reaction),
		)
		.route("/channels/{id}/messages/{mid}/ack", post(channels::ack))
		.route(
			"/channels/{id}/messages/{mid}/threads",
			post(channels::thread_from_message),
		)
		.route("/channels/{id}/pins", get(channels::pins))
		.route("/channels/{id}/pins/{mid}", put(admin::pin).delete(admin::unpin))
		.route(
			"/channels/{id}/permissions/{tid}",
			put(admin::put_overwrite).delete(admin::delete_overwrite),
		)
		.route(
			"/channels/{id}/thread-members/@me",
			put(admin::join_thread).delete(admin::leave_thread),
		)
		.route(
			"/channels/{id}/recipients/{uid}",
			put(admin::add_recipient).delete(admin::remove_recipient),
		)
		.route("/channels/{id}/invites", post(admin::create_invite))
		.route("/channels/{id}/polls/{mid}/answers/@me", put(admin::vote))
		.route("/channels/{id}/polls/{mid}/expire", post(admin::expire_poll))
		.route("/channels/{id}/typing", post(channels::typing))
		.route("/channels/{id}/call/ring", post(channels::call_signal))
		.route("/interactions", post(channels::interaction))
		.route("/guilds/{id}/application-command-index", get(apps::guild_index))
		.route(
			"/channels/{id}/application-command-index",
			get(apps::channel_index),
		)
		.route("/users/@me/application-command-index", get(apps::user_index))
		.route("/users/@me/connections", get(rpc::connections))
		.route("/applications/detectable", get(rpc::detectable))
		.route("/applications/{id}/rpc", get(rpc::rpc_application))
		.route("/applications/{id}/external-assets", post(rpc::external_assets))
		.route("/oauth2/applications/{id}/assets", get(rpc::application_assets))
		.route("/channels/{id}/call/stop-ringing", post(channels::call_signal))
		.route("/channels/{id}/threads", post(channels::create_thread))
		.route(
			"/channels/{id}/threads/archived/public",
			get(admin::archived_threads),
		)
		.merge(expressions::routes())
		.route_layer(middleware::from_fn_with_state(shared.clone(), guard));
	Router::new()
		.route("/gateway", get(gateway::upgrade))
		.route("/media/{name}", get(media::media))
		.nest("/api/v10", api)
		.fallback(not_found)
		.layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
		.with_state(shared)
}

async fn not_found() -> ApiError {
	ApiError::not_found()
}

fn chaos_rate_limited() -> bool {
	rand::rng().random_bool(CHAOS_RATE_LIMIT_PERCENT)
}

fn rate_limit_response() -> Response {
	let body = RateLimitBody {
		message: "You are being rate limited.".to_owned(),
		retry_after: CHAOS_RETRY_AFTER,
		global: false,
		code: Some(0),
	};
	let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
	response
		.headers_mut()
		.insert("retry-after", HeaderValue::from_static("1"));
	response
}

fn set_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
	if let Ok(value) = HeaderValue::from_str(value) {
		headers.insert(HeaderName::from_static(name), value);
	}
}

fn apply_rate_limit_headers(headers: &mut HeaderMap, key: &str, snapshot: &RateLimitSnapshot, limited: bool) {
	let mut hasher = DefaultHasher::new();
	key.hash(&mut hasher);
	let reset_at = Utc::now().timestamp_millis() as f64 / 1000.0 + snapshot.reset_after;
	let remaining = if limited { 0 } else { snapshot.remaining };
	set_header(headers, "x-ratelimit-limit", &BUCKET_LIMIT.to_string());
	set_header(headers, "x-ratelimit-remaining", &remaining.to_string());
	set_header(
		headers,
		"x-ratelimit-reset-after",
		&format!("{:.3}", snapshot.reset_after),
	);
	set_header(headers, "x-ratelimit-reset", &format!("{reset_at:.3}"));
	set_header(
		headers,
		"x-ratelimit-bucket",
		&format!("{:016x}", hasher.finish()),
	);
}

async fn guard(State(shared): State<Arc<Shared>>, request: Request, next: Next) -> Response {
	let route = request
		.extensions()
		.get::<MatchedPath>()
		.map(|path| path.as_str().to_owned())
		.unwrap_or_default();
	let key = format!("{} {route}", request.method());
	let authorized = route == "/api/v10/applications/detectable"
		|| request
			.headers()
			.get(AUTHORIZATION)
			.and_then(|value| value.to_str().ok())
			.is_some_and(|value| value == shared.config.token);
	let snapshot = shared.touch_bucket(&key);
	let limited = authorized && shared.config.chaos && chaos_rate_limited();
	let mut response = if !authorized {
		ApiError::unauthorized().into_response()
	} else if limited {
		rate_limit_response()
	} else {
		next.run(request).await
	};
	apply_rate_limit_headers(response.headers_mut(), &key, &snapshot, limited);
	response
}
