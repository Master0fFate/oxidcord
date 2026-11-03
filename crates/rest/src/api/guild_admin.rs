use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use protocol::{
	AutoModRule, AutoModTriggerType, Channel, ChannelPosition, LenientVec, Role, ScheduledEvent,
	ScheduledEventRsvp, Snowflake, StageInstance,
};
use serde_json::{Map, Value, json};

use crate::client::RestClient;
use crate::error::Result;
use crate::request::Request;

pub(super) fn with_reason(request: Request, reason: Option<&str>) -> Request {
	match reason.filter(|reason| !reason.is_empty()) {
		Some(reason) => request.audit_reason(reason),
		None => request,
	}
}

fn automod_metadata(rule: &AutoModRule) -> Value {
	let metadata = &rule.trigger_metadata;
	match rule.trigger_type {
		AutoModTriggerType::Keyword | AutoModTriggerType::MemberProfile => json!({
			"keyword_filter": metadata.keyword_filter,
			"regex_patterns": metadata.regex_patterns,
			"allow_list": metadata.allow_list,
		}),
		AutoModTriggerType::KeywordPreset => json!({
			"presets": metadata.presets,
			"allow_list": metadata.allow_list,
		}),
		AutoModTriggerType::MentionSpam => json!({
			"mention_total_limit": metadata.mention_total_limit,
			"mention_raid_protection_enabled": metadata.mention_raid_protection_enabled,
		}),
		AutoModTriggerType::Spam | AutoModTriggerType::Unknown(_) => Value::Null,
	}
}

fn automod_body(rule: &AutoModRule) -> Value {
	let mut body = json!({
		"name": rule.name,
		"event_type": rule.event_type,
		"actions": rule.actions,
		"enabled": rule.enabled,
		"exempt_roles": rule.exempt_roles,
		"exempt_channels": rule.exempt_channels,
	});
	let metadata = automod_metadata(rule);
	if !metadata.is_null() {
		body["trigger_metadata"] = metadata;
	}
	body
}

fn automod_changes(rule: &AutoModRule, original: &AutoModRule) -> Value {
	let mut changes = Map::new();
	if rule.name != original.name {
		changes.insert("name".to_owned(), json!(rule.name));
	}
	if rule.event_type != original.event_type {
		changes.insert("event_type".to_owned(), json!(rule.event_type));
	}
	if rule.actions != original.actions {
		changes.insert("actions".to_owned(), json!(rule.actions));
	}
	if rule.enabled != original.enabled {
		changes.insert("enabled".to_owned(), json!(rule.enabled));
	}
	if rule.exempt_roles != original.exempt_roles {
		changes.insert("exempt_roles".to_owned(), json!(rule.exempt_roles));
	}
	if rule.exempt_channels != original.exempt_channels {
		changes.insert("exempt_channels".to_owned(), json!(rule.exempt_channels));
	}
	let metadata = automod_metadata(rule);
	if rule.trigger_metadata != original.trigger_metadata && !metadata.is_null() {
		changes.insert("trigger_metadata".to_owned(), metadata);
	}
	Value::Object(changes)
}

impl RestClient {
	pub async fn modify_guild(&self, guild_id: Snowflake, changes: Value) -> Result<Value> {
		self.send(Request::patch(format!("/guilds/{guild_id}")).json(&changes))
			.await
	}

	pub async fn delete_guild(&self, guild_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/guilds/{guild_id}")))
			.await
	}

	pub async fn request_ownership_code(&self, guild_id: Snowflake) -> Result<()> {
		self.send_empty(Request::put(format!("/guilds/{guild_id}/pincode")))
			.await
	}

	pub async fn transfer_guild(
		&self,
		guild_id: Snowflake,
		owner_id: Snowflake,
		code: Option<&str>,
	) -> Result<Value> {
		let mut changes = json!({ "owner_id": owner_id });
		if let Some(code) = code {
			changes["code"] = json!(code);
		}
		self.modify_guild(guild_id, changes).await
	}

	pub async fn create_guild_channel(&self, guild_id: Snowflake, body: Value) -> Result<Channel> {
		self.send(Request::post(format!("/guilds/{guild_id}/channels")).json(&body))
			.await
	}

	pub async fn modify_channel_positions(
		&self,
		guild_id: Snowflake,
		positions: &[ChannelPosition],
	) -> Result<()> {
		self.send_empty(Request::patch(format!("/guilds/{guild_id}/channels")).json(&positions))
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

	pub async fn automod_rules(&self, guild_id: Snowflake) -> Result<Value> {
		self.send(Request::get(format!("/guilds/{guild_id}/auto-moderation/rules")))
			.await
	}

	pub async fn save_automod_rule(
		&self,
		guild_id: Snowflake,
		rule: &AutoModRule,
		original: Option<&AutoModRule>,
	) -> Result<Value> {
		let request = match original {
			Some(original) => Request::patch(format!("/guilds/{guild_id}/auto-moderation/rules/{}", rule.id))
				.json(&automod_changes(rule, original)),
			None => {
				let mut body = automod_body(rule);
				body["trigger_type"] = json!(rule.trigger_type);
				Request::post(format!("/guilds/{guild_id}/auto-moderation/rules")).json(&body)
			}
		};
		self.send(request).await
	}

	pub async fn set_automod_rule_enabled(
		&self,
		guild_id: Snowflake,
		rule_id: Snowflake,
		enabled: bool,
	) -> Result<Value> {
		let request = Request::patch(format!("/guilds/{guild_id}/auto-moderation/rules/{rule_id}"))
			.json(&json!({ "enabled": enabled }));
		self.send(request).await
	}

	pub async fn delete_automod_rule(&self, guild_id: Snowflake, rule_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!(
			"/guilds/{guild_id}/auto-moderation/rules/{rule_id}"
		)))
		.await
	}

	pub async fn guild_bans(
		&self,
		guild_id: Snowflake,
		after: Option<Snowflake>,
		limit: usize,
	) -> Result<Value> {
		let mut request =
			Request::get(format!("/guilds/{guild_id}/bans")).query(&[("limit", limit.to_string())]);
		if let Some(after) = after {
			request = request.query(&[("after", after.to_string())]);
		}
		self.send(request).await
	}

	pub async fn role_member_counts(&self, guild_id: Snowflake) -> Result<Value> {
		self.send(Request::get(format!("/guilds/{guild_id}/roles/member-counts")))
			.await
	}

	pub async fn guild_invites(&self, guild_id: Snowflake) -> Result<Value> {
		self.send(Request::get(format!("/guilds/{guild_id}/invites")))
			.await
	}

	pub async fn scheduled_events(&self, guild_id: Snowflake) -> Result<LenientVec<ScheduledEvent>> {
		let request = Request::get(format!("/guilds/{guild_id}/scheduled-events"))
			.query(&[("with_user_count", "true")]);
		self.send(request).await
	}

	pub async fn scheduled_event_rsvps(&self, guild_id: Snowflake) -> Result<LenientVec<ScheduledEventRsvp>> {
		let request =
			Request::get("/users/@me/scheduled-events").query(&[("guild_ids", guild_id.to_string())]);
		self.send(request).await
	}

	pub async fn create_scheduled_event(&self, guild_id: Snowflake, body: Value) -> Result<ScheduledEvent> {
		self.send(Request::post(format!("/guilds/{guild_id}/scheduled-events")).json(&body))
			.await
	}

	pub async fn modify_scheduled_event(
		&self,
		guild_id: Snowflake,
		event_id: Snowflake,
		changes: Value,
	) -> Result<ScheduledEvent> {
		let request =
			Request::patch(format!("/guilds/{guild_id}/scheduled-events/{event_id}")).json(&changes);
		self.send(request).await
	}

	pub async fn delete_scheduled_event(&self, guild_id: Snowflake, event_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!(
			"/guilds/{guild_id}/scheduled-events/{event_id}"
		)))
		.await
	}

	pub async fn set_scheduled_event_interest(
		&self,
		guild_id: Snowflake,
		event_id: Snowflake,
		interested: bool,
	) -> Result<()> {
		let path = format!("/guilds/{guild_id}/scheduled-events/{event_id}/users/@me");
		let request = if interested {
			Request::put(path)
		} else {
			Request::delete(path)
		};
		self.send_empty(request).await
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

	pub async fn create_stage_instance(&self, channel_id: Snowflake, topic: &str) -> Result<StageInstance> {
		let request = Request::post("/stage-instances").json(&json!({
			"channel_id": channel_id,
			"topic": topic,
			"privacy_level": 2,
		}));
		self.send(request).await
	}

	pub async fn modify_stage_instance(&self, channel_id: Snowflake, topic: &str) -> Result<StageInstance> {
		let request =
			Request::patch(format!("/stage-instances/{channel_id}")).json(&json!({ "topic": topic }));
		self.send(request).await
	}

	pub async fn delete_stage_instance(&self, channel_id: Snowflake) -> Result<()> {
		self.send_empty(Request::delete(format!("/stage-instances/{channel_id}")))
			.await
	}

	pub async fn modify_voice_state(
		&self,
		guild_id: Snowflake,
		user_id: Option<Snowflake>,
		body: Value,
	) -> Result<()> {
		let target = user_id.map_or_else(|| "@me".to_owned(), |user_id| user_id.to_string());
		let request = Request::patch(format!("/guilds/{guild_id}/voice-states/{target}")).json(&body);
		self.send_empty(request).await
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
