use protocol::{Guild, LenientVec, Message, Relationship, Snowflake, User};
use serde_json::{Value, json};

use crate::client::RestClient;
use crate::error::Result;
use crate::request::Request;

impl RestClient {
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

	pub async fn mentions(&self, limit: u8) -> Result<LenientVec<Message>> {
		let request = Request::get("/users/@me/mentions").query(&[
			("limit", limit.to_string().as_str()),
			("roles", "true"),
			("everyone", "true"),
		]);
		self.send(request).await
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
	pub async fn send_friend_request(&self, username: &str) -> Result<()> {
		let request = Request::post("/users/@me/relationships")
			.json(&json!({ "username": username, "discriminator": null }));
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
