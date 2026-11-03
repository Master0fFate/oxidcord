use protocol::{Channel, Emoji, Guild, LenientVec, Member, MemberVerification, Role, Snowflake};
use serde_json::json;

use crate::client::RestClient;
use crate::error::Result;
use crate::request::Request;

impl RestClient {
	pub async fn guild(&self, id: Snowflake) -> Result<Guild> {
		self.send(Request::get(format!("/guilds/{id}"))).await
	}

	pub async fn guild_channels(&self, id: Snowflake) -> Result<LenientVec<Channel>> {
		self.send(Request::get(format!("/guilds/{id}/channels"))).await
	}

	pub async fn guild_members_search(
		&self,
		id: Snowflake,
		query: &str,
		limit: u8,
	) -> Result<LenientVec<Member>> {
		let request = Request::get(format!("/guilds/{id}/members/search"))
			.query(&[("query", query.to_owned()), ("limit", limit.to_string())]);
		self.send(request).await
	}

	pub async fn guild_member(&self, id: Snowflake, user_id: Snowflake) -> Result<Member> {
		self.send(Request::get(format!("/guilds/{id}/members/{user_id}")))
			.await
	}

	pub async fn guild_roles(&self, id: Snowflake) -> Result<LenientVec<Role>> {
		self.send(Request::get(format!("/guilds/{id}/roles"))).await
	}

	pub async fn guild_emojis(&self, id: Snowflake) -> Result<LenientVec<Emoji>> {
		self.send(Request::get(format!("/guilds/{id}/emojis"))).await
	}

	pub async fn create_guild(&self, name: &str, icon: Option<&str>) -> Result<Guild> {
		let request = Request::post("/guilds").json(&json!({ "name": name, "icon": icon }));
		self.send(request).await
	}

	pub async fn leave_guild(&self, id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/users/@me/guilds/{id}")))
			.await
	}

	pub async fn guild_preview(&self, id: Snowflake) -> Result<Guild> {
		self.send(Request::get(format!("/guilds/{id}/preview"))).await
	}

	pub async fn member_verification(
		&self,
		guild_id: Snowflake,
		invite_code: Option<&str>,
	) -> Result<MemberVerification> {
		let mut query = vec![("with_guild", "false")];
		query.extend(invite_code.map(|code| ("invite_code", code)));
		self.send(Request::get(format!("/guilds/{guild_id}/member-verification")).query(&query))
			.await
	}

	pub async fn join_request_guilds(&self) -> Result<LenientVec<Guild>> {
		self.send(Request::get("/users/@me/join-request-guilds")).await
	}

	pub async fn delete_join_request(&self, guild_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/guilds/{guild_id}/requests/@me")))
			.await
	}

	pub async fn submit_join_request(&self, guild_id: Snowflake, form: &MemberVerification) -> Result<()> {
		let body = json!({ "version": form.version, "form_fields": form.form_fields });
		self.send_empty(Request::put(format!("/guilds/{guild_id}/requests/@me")).json(&body))
			.await
	}
}
