use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use protocol::{Channel, LenientVec, Role, Snowflake};
use serde_json::{Value, json};

use crate::client::RestClient;
use crate::error::Result;
use crate::request::Request;

pub(super) fn with_reason(request: Request, reason: Option<&str>) -> Request {
	match reason.filter(|reason| !reason.is_empty()) {
		Some(reason) => request.audit_reason(reason),
		None => request,
	}
}

impl RestClient {
	pub async fn modify_guild(&self, guild_id: Snowflake, changes: Value) -> Result<Value> {
		self.send(Request::patch(format!("/guilds/{guild_id}")).json(&changes))
			.await
	}

	pub async fn create_guild_channel(&self, guild_id: Snowflake, body: Value) -> Result<Channel> {
		self.send(Request::post(format!("/guilds/{guild_id}/channels")).json(&body))
			.await
	}

	pub async fn create_role(&self, guild_id: Snowflake, body: Value) -> Result<Role> {
		self.send(Request::post(format!("/guilds/{guild_id}/roles")).json(&body))
			.await
	}

	pub async fn modify_role(&self, guild_id: Snowflake, role_id: Snowflake, body: Value) -> Result<Role> {
		self.send(Request::patch(format!("/guilds/{guild_id}/roles/{role_id}")).json(&body))
			.await
	}

	pub async fn delete_role(&self, guild_id: Snowflake, role_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/guilds/{guild_id}/roles/{role_id}")))
			.await
	}

	pub async fn reorder_roles(
		&self,
		guild_id: Snowflake,
		positions: &[(Snowflake, i64)],
	) -> Result<LenientVec<Role>> {
		let body: Vec<Value> = positions
			.iter()
			.map(|(id, position)| json!({ "id": id, "position": position }))
			.collect();
		self.send(Request::patch(format!("/guilds/{guild_id}/roles")).json(&body))
			.await
	}

	pub async fn create_emoji(&self, guild_id: Snowflake, name: &str, image: &str) -> Result<Value> {
		let request = Request::post(format!("/guilds/{guild_id}/emojis"))
			.json(&json!({ "name": name, "image": image }));
		self.send(request).await
	}

	pub async fn delete_emoji(&self, guild_id: Snowflake, emoji_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/guilds/{guild_id}/emojis/{emoji_id}")))
			.await
	}

	pub async fn audit_log(
		&self,
		guild_id: Snowflake,
		before: Option<Snowflake>,
		action_type: Option<i64>,
	) -> Result<Value> {
		let mut request = Request::get(format!("/guilds/{guild_id}/audit-logs")).query(&[("limit", "50")]);
		if let Some(before) = before {
			request = request.query(&[("before", before.to_string())]);
		}
		if let Some(action_type) = action_type {
			request = request.query(&[("action_type", action_type.to_string())]);
		}
		self.send(request).await
	}

	pub async fn guild_bans(&self, guild_id: Snowflake) -> Result<Value> {
		let request = Request::get(format!("/guilds/{guild_id}/bans")).query(&[("limit", "1000")]);
		self.send(request).await
	}

	pub async fn guild_invites(&self, guild_id: Snowflake) -> Result<Value> {
		self.send(Request::get(format!("/guilds/{guild_id}/invites")))
			.await
	}

	pub async fn kick_member(
		&self,
		guild_id: Snowflake,
		user_id: Snowflake,
		reason: Option<&str>,
	) -> Result<()> {
		let request = Request::delete(format!("/guilds/{guild_id}/members/{user_id}"));
		self.send_empty(with_reason(request, reason)).await
	}

	pub async fn ban_member(
		&self,
		guild_id: Snowflake,
		user_id: Snowflake,
		delete_message_seconds: u32,
		reason: Option<&str>,
	) -> Result<()> {
		let request = Request::put(format!("/guilds/{guild_id}/bans/{user_id}"))
			.json(&json!({ "delete_message_seconds": delete_message_seconds }));
		self.send_empty(with_reason(request, reason)).await
	}

	pub async fn unban_member(&self, guild_id: Snowflake, user_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/guilds/{guild_id}/bans/{user_id}")))
			.await
	}

	pub async fn modify_member(
		&self,
		guild_id: Snowflake,
		user_id: Snowflake,
		changes: Value,
	) -> Result<Value> {
		self.send(Request::patch(format!("/guilds/{guild_id}/members/{user_id}")).json(&changes))
			.await
	}

	pub async fn modify_current_member(&self, guild_id: Snowflake, changes: Value) -> Result<Value> {
		self.send(Request::patch(format!("/guilds/{guild_id}/members/@me")).json(&changes))
			.await
	}

	pub async fn add_member_role(
		&self,
		guild_id: Snowflake,
		user_id: Snowflake,
		role_id: Snowflake,
	) -> Result<()> {
		self.send_empty(Request::put(format!(
			"/guilds/{guild_id}/members/{user_id}/roles/{role_id}"
		)))
		.await
	}

	pub async fn remove_member_role(
		&self,
		guild_id: Snowflake,
		user_id: Snowflake,
		role_id: Snowflake,
	) -> Result<()> {
		self.send_empty(Request::delete(format!(
			"/guilds/{guild_id}/members/{user_id}/roles/{role_id}"
		)))
		.await
	}
}

pub fn image_data_uri(mime: &str, bytes: &[u8]) -> String {
	let mut uri = String::with_capacity(bytes.len() * 4 / 3 + mime.len() + 16);
	uri.push_str("data:");
	uri.push_str(mime);
	uri.push_str(";base64,");
	STANDARD.encode_string(bytes, &mut uri);
	uri
}
