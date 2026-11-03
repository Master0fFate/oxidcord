use protocol::{Connection, Guild, LenientVec, Message, Relationship, Snowflake, User};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::RestClient;
use crate::error::{RestError, Result};
use crate::request::{Request, encode_component};

#[derive(Deserialize)]
struct ActivitySecret {
	secret: String,
}

impl RestClient {
	pub async fn connections(&self) -> Result<LenientVec<Connection>> {
		self.send(Request::get("/users/@me/connections")).await
	}

	pub async fn connection_access_token(&self, kind: &str, id: &str) -> Result<Value> {
		let path = format!(
			"/users/@me/connections/{}/{}/access-token",
			encode_component(kind),
			encode_component(id)
		);
		self.send(Request::get(path)).await
	}

	pub async fn application_assets(&self, id: Snowflake) -> Result<Value> {
		self.send(Request::get(format!("/oauth2/applications/{id}/assets")))
			.await
	}

	pub async fn external_assets(&self, id: Snowflake, urls: &[String]) -> Result<Value> {
		let request =
			Request::post(format!("/applications/{id}/external-assets")).json(&json!({ "urls": urls }));
		self.send(request).await
	}

	pub async fn current_user(&self) -> Result<User> {
		self.send(Request::get("/users/@me")).await
	}

	pub async fn user(&self, id: Snowflake) -> Result<User> {
		self.send(Request::get(format!("/users/{id}"))).await
	}

	pub async fn user_profile(&self, id: Snowflake, guild_id: Option<Snowflake>) -> Result<Value> {
		let mut request = Request::get(format!("/users/{id}/profile")).query(&[
			("with_mutual_guilds", "true"),
			("with_mutual_friends", "true"),
			("with_mutual_friends_count", "true"),
		]);
		if let Some(guild_id) = guild_id {
			request = request.query(&[("guild_id", guild_id.to_string())]);
		}
		self.send(request).await
	}

	pub async fn application_rpc(&self, id: Snowflake) -> Result<Value> {
		self.send(Request::get(format!("/applications/{id}/rpc"))).await
	}

	pub async fn activity_secret(
		&self,
		user_id: Snowflake,
		session_id: &str,
		application_id: Snowflake,
		kind: i64,
		invite: Option<(Snowflake, Snowflake)>,
	) -> Result<String> {
		let mut request = Request::get(format!(
			"/users/{user_id}/sessions/{}/activities/{application_id}/{kind}",
			encode_component(session_id)
		));
		if let Some((channel_id, message_id)) = invite {
			request = request.query(&[
				("channel_id", channel_id.to_string()),
				("message_id", message_id.to_string()),
			]);
		}
		let secret: ActivitySecret = self.send(request).await?;
		Ok(secret.secret)
	}

	pub async fn mentions(&self, limit: u8, roles: bool, everyone: bool) -> Result<LenientVec<Message>> {
		let request = Request::get("/users/@me/mentions").query(&[
			("limit", limit.to_string().as_str()),
			("roles", if roles { "true" } else { "false" }),
			("everyone", if everyone { "true" } else { "false" }),
		]);
		self.send(request).await
	}

	pub async fn delete_mention(&self, message_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/users/@me/mentions/{message_id}")))
			.await
	}

	pub async fn relationships(&self) -> Result<LenientVec<Relationship>> {
		self.send(Request::get("/users/@me/relationships")).await
	}

	pub async fn add_relationship(&self, user_id: Snowflake) -> Result<()> {
		let request = Request::put(format!("/users/@me/relationships/{user_id}")).json(&json!({}));
		self.send_empty(request).await
	}

	pub async fn remove_relationship(&self, user_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/users/@me/relationships/{user_id}")))
			.await
	}

	pub async fn set_ignored(&self, user_id: Snowflake, ignored: bool) -> Result<()> {
		let path = format!("/users/@me/relationships/{user_id}/ignore");
		let request = if ignored {
			Request::put(path).json(&json!({}))
		} else {
			Request::delete(path)
		};
		self.send_empty(request).await
	}

	pub async fn set_friend_nickname(&self, user_id: Snowflake, nickname: Option<&str>) -> Result<()> {
		let request = Request::patch(format!("/users/@me/relationships/{user_id}"))
			.json(&json!({ "nickname": nickname }));
		self.send_empty(request).await
	}

	pub async fn user_note(&self, user_id: Snowflake) -> Result<String> {
		match self
			.send::<Value>(Request::get(format!("/users/@me/notes/{user_id}")))
			.await
		{
			Ok(found) => Ok(found["note"].as_str().unwrap_or_default().to_owned()),
			Err(RestError::NotFound(_)) => Ok(String::new()),
			Err(error) => Err(error),
		}
	}

	pub async fn set_user_note(&self, user_id: Snowflake, note: &str) -> Result<()> {
		let request = Request::put(format!("/users/@me/notes/{user_id}")).json(&json!({ "note": note }));
		self.send_empty(request).await
	}

	pub async fn current_user_guilds(&self) -> Result<LenientVec<Guild>> {
		self.send(Request::get("/users/@me/guilds")).await
	}

	pub async fn user_settings(&self) -> Result<Value> {
		self.send(Request::get("/users/@me/settings")).await
	}

	pub async fn patch_user_settings(&self, settings: Value) -> Result<Value> {
		self.send(Request::patch("/users/@me/settings").json(&settings))
			.await
	}

	pub async fn auth_sessions(&self) -> Result<Value> {
		self.send(Request::get("/auth/sessions")).await
	}

	pub async fn logout_sessions(&self, session_id_hashes: &[String]) -> Result<()> {
		let request =
			Request::post("/auth/sessions/logout").json(&json!({ "session_id_hashes": session_id_hashes }));
		self.send_empty(request).await
	}
}

impl RestClient {
	pub async fn send_friend_request(&self, username: &str, discriminator: Option<u16>) -> Result<()> {
		let request = Request::post("/users/@me/relationships")
			.json(&json!({ "username": username, "discriminator": discriminator }));
		self.send_empty(request).await
	}

	pub async fn block_user(&self, user_id: Snowflake) -> Result<()> {
		let request = Request::put(format!("/users/@me/relationships/{user_id}")).json(&json!({ "type": 2 }));
		self.send_empty(request).await
	}

	pub async fn patch_current_user(&self, changes: Value) -> Result<User> {
		self.send(Request::patch("/users/@me").json(&changes)).await
	}

	pub async fn patch_current_profile(&self, changes: Value) -> Result<Value> {
		self.send(Request::patch("/users/@me/profile").json(&changes))
			.await
	}

	pub async fn patch_guild_settings(&self, guild_id: Option<Snowflake>, changes: Value) -> Result<Value> {
		let scope = guild_id.map_or_else(|| "@me".to_owned(), |id| id.to_string());
		let request = Request::patch(format!("/users/@me/guilds/{scope}/settings")).json(&changes);
		self.send(request).await
	}

	pub async fn ack_bulk(&self, read_states: Vec<(Snowflake, Snowflake)>) -> Result<()> {
		let entries: Vec<Value> = read_states
			.into_iter()
			.map(|(channel_id, message_id)| {
				json!({ "channel_id": channel_id, "message_id": message_id, "read_state_type": 0 })
			})
			.collect();
		let request = Request::post("/read-states/ack-bulk").json(&json!({ "read_states": entries }));
		self.send_empty(request).await
	}
}
