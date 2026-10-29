use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use protocol::{Connection, Snowflake};
use reqwest::StatusCode;
use rest::RestClient;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, interval, sleep, sleep_until, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::engine::Input;

const DEALER_URL: &str = "wss://dealer.spotify.com/";
const SUBSCRIBE_URL: &str = "https://api.spotify.com/v1/me/notifications/player";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const PING_INTERVAL: Duration = Duration::from_secs(30);
const RETRY_MIN: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(5 * 60);
const SPOTIFY_FLAGS: u64 = 48;
const END_GRACE: Duration = Duration::from_secs(5);
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpotifyAccount {
	pub connection_id: String,
	pub token: String,
	pub user_id: Snowflake,
}

enum Outcome {
	Unauthorized,
	Failed,
}

fn first_text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
	value.get(key)?.as_str().filter(|text| !text.is_empty())
}

fn build(state: &Value, user_id: Snowflake) -> Option<Value> {
	if state.get("is_playing").and_then(Value::as_bool) != Some(true) {
		return None;
	}
	let item = state.get("item")?;
	let track_id = first_text(item, "id")?;
	let track_name = first_text(item, "name")?;
	let duration = item.get("duration_ms")?.as_i64()?;
	let progress = state.get("progress_ms").and_then(Value::as_i64).unwrap_or(0);
	let artists: Vec<&Value> = item
		.get("artists")
		.and_then(Value::as_array)
		.map(|artists| artists.iter().collect())
		.unwrap_or_default();
	let artist_names: Vec<&str> = artists
		.iter()
		.filter_map(|artist| first_text(artist, "name"))
		.collect();
	let artist_ids: Vec<&str> = artists
		.iter()
		.filter_map(|artist| first_text(artist, "id"))
		.collect();
	let album = item.get("album");
	let album_name = album.and_then(|album| first_text(album, "name"));
	let album_id = album.and_then(|album| first_text(album, "id"));
	let image_hash = album
		.and_then(|album| album.get("images")?.as_array()?.first())
		.and_then(|image| first_text(image, "url"))
		.and_then(|url| url.rsplit('/').next())
		.filter(|hash| !hash.is_empty());
	let start = Utc::now().timestamp_millis() - progress;
	let mut activity = json!({
		"type": 2,
		"name": "Spotify",
		"details": track_name,
		"state": artist_names.join("; "),
		"timestamps": { "start": start, "end": start + duration },
		"party": { "id": format!("spotify:{user_id}") },
		"sync_id": track_id,
		"flags": SPOTIFY_FLAGS,
		"metadata": { "album_id": album_id, "artist_ids": artist_ids },
	});
	if let Some(hash) = image_hash {
		activity["assets"] = json!({ "large_image": format!("spotify:{hash}"), "large_text": album_name });
	}
	if let Some(context) = state
		.get("context")
		.and_then(|context| first_text(context, "uri"))
	{
		activity["metadata"]["context_uri"] = Value::String(context.to_owned());
	}
	Some(activity)
}

async fn subscribe(http: &reqwest::Client, token: &str, connection_id: &str) -> Result<(), Outcome> {
	let url = Url::parse_with_params(SUBSCRIBE_URL, [("connection_id", connection_id)])
		.map_err(|_| Outcome::Failed)?;
	let response = http
		.put(url)
		.bearer_auth(token)
		.send()
		.await
		.map_err(|_| Outcome::Failed)?;
	match response.status() {
		status if status.is_success() => Ok(()),
		StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(Outcome::Unauthorized),
		_ => Err(Outcome::Failed),
	}
}

fn events(message: &Value) -> impl Iterator<Item = &Value> {
	message
		.get("payloads")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|payload| payload.get("events")?.as_array())
		.flatten()
}

fn ends_at(state: &Value) -> Option<Instant> {
	let duration = state.get("item")?.get("duration_ms")?.as_i64()?;
	let progress = state.get("progress_ms").and_then(Value::as_i64).unwrap_or(0);
	let remaining = u64::try_from(duration - progress).unwrap_or(0);
	Some(Instant::now() + Duration::from_millis(remaining) + END_GRACE)
}

fn no_active_device(event: &Value) -> bool {
	event
		.get("event")
		.and_then(|event| event.get("devices"))
		.and_then(Value::as_array)
		.is_some_and(|devices| {
			!devices
				.iter()
				.any(|device| device.get("is_active").and_then(Value::as_bool) == Some(true))
		})
}

struct Player<'a> {
	account: &'a SpotifyAccount,
	inputs: &'a mpsc::UnboundedSender<Input>,
	expiry: Option<Instant>,
}

impl Player<'_> {
	fn publish(&mut self, activity: Option<Value>, expiry: Option<Instant>) {
		self.expiry = activity.is_some().then_some(expiry).flatten();
		let _ = self.inputs.send(Input::Spotify(activity));
	}

	fn apply(&mut self, message: &Value) {
		for event in events(message) {
			match event.get("type").and_then(Value::as_str) {
				Some("PLAYER_STATE_CHANGED") => {
					if let Some(state) = event.get("event").and_then(|event| event.get("state")) {
						self.publish(build(state, self.account.user_id), ends_at(state));
					}
				}
				Some("DEVICE_STATE_CHANGED") if no_active_device(event) => self.publish(None, None),
				_ => {}
			}
		}
	}
}

async fn session(
	http: &reqwest::Client,
	account: &SpotifyAccount,
	token: &str,
	inputs: &mpsc::UnboundedSender<Input>,
) -> Outcome {
	let url = format!("{DEALER_URL}?access_token={token}");
	let connected = timeout(CONNECT_TIMEOUT, connect_async(url)).await;
	let (mut socket, _) = match connected {
		Ok(Ok(connected)) => connected,
		Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response)))
			if matches!(
				response.status(),
				StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
			) =>
		{
			return Outcome::Unauthorized;
		}
		_ => return Outcome::Failed,
	};
	let mut pings = interval(PING_INTERVAL);
	pings.tick().await;
	let mut subscribed = false;
	let mut player = Player {
		account,
		inputs,
		expiry: None,
	};
	loop {
		let expiry = player.expiry;
		tokio::select! {
			incoming = socket.next() => {
				let text = match incoming {
					Some(Ok(Message::Text(text))) if text.len() <= MAX_MESSAGE_BYTES => text,
					Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return Outcome::Failed,
					Some(Ok(_)) => continue,
				};
				let Ok(message) = serde_json::from_str::<Value>(&text) else {
					continue;
				};
				if let Some(connection_id) = message
					.get("headers")
					.and_then(|headers| headers.get("Spotify-Connection-Id"))
					.and_then(Value::as_str)
				{
					if let Err(outcome) = subscribe(http, token, connection_id).await {
						return outcome;
					}
					subscribed = true;
					tracing::debug!("spotify player events subscribed");
					continue;
				}
				if subscribed {
					player.apply(&message);
				}
			}
			() = sleep_until(expiry.unwrap_or_else(Instant::now)), if expiry.is_some() => {
				player.publish(None, None);
			}
			_ = pings.tick() => {
				if socket.send(Message::text(r#"{"type":"ping"}"#)).await.is_err() {
					return Outcome::Failed;
				}
			}
		}
	}
}

async fn refresh_token(rest: &RestClient, account: &SpotifyAccount) -> Option<String> {
	let response = rest
		.connection_access_token(Connection::SPOTIFY, &account.connection_id)
		.await
		.ok()?;
	response
		.get("access_token")?
		.as_str()
		.filter(|token| !token.is_empty())
		.map(str::to_owned)
}

pub(crate) async fn run(
	rest: RestClient,
	http: reqwest::Client,
	mut account: watch::Receiver<Option<SpotifyAccount>>,
	inputs: mpsc::UnboundedSender<Input>,
) {
	let mut token: Option<String> = None;
	let mut retry = RETRY_MIN;
	loop {
		let Some(current) = account.borrow().clone() else {
			if account.changed().await.is_err() {
				return;
			}
			token = None;
			retry = RETRY_MIN;
			continue;
		};
		let active = token.clone().unwrap_or_else(|| current.token.clone());
		let outcome = tokio::select! {
			outcome = session(&http, &current, &active, &inputs) => Some(outcome),
			changed = account.changed() => {
				if changed.is_err() {
					return;
				}
				None
			}
		};
		let _ = inputs.send(Input::Spotify(None));
		let Some(outcome) = outcome else {
			token = None;
			retry = RETRY_MIN;
			continue;
		};
		if matches!(outcome, Outcome::Unauthorized) || token.is_none() {
			token = refresh_token(&rest, &current).await;
		}
		tokio::select! {
			() = sleep(retry) => {}
			changed = account.changed() => {
				if changed.is_err() {
					return;
				}
				token = None;
				retry = RETRY_MIN;
				continue;
			}
		}
		retry = (retry * 2).min(RETRY_MAX);
	}
}
