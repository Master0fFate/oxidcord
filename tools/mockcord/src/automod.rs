use std::collections::HashMap;

use protocol::{
	AutoModAction, AutoModActionType, AutoModEventType, AutoModRule, AutoModTriggerType, LenientVec,
	Snowflake,
};
use serde_json::{Value, json};

use crate::admin::{ACTION_AUTOMOD_RULE_CREATE, ACTION_AUTOMOD_RULE_DELETE, ACTION_AUTOMOD_RULE_UPDATE};
use crate::error::ApiError;
use crate::fixtures::{CURRENT_USER_ID, Fixtures};
use crate::ids::snowflake_at;
use crate::json::to_json;
use crate::state::Shared;

const SEED_RULE_AGE_MS: i64 = 86_400_000;
const SEED_TIMEOUT_SECONDS: i64 = 3600;
const SEED_MENTION_LIMIT: i64 = 8;

fn action(kind: AutoModActionType) -> AutoModAction {
	AutoModAction {
		kind,
		..AutoModAction::default()
	}
}

fn alert(channel_id: u64) -> AutoModAction {
	let mut alert = action(AutoModActionType::SendAlert);
	alert.metadata.channel_id = Some(Snowflake(channel_id));
	alert
}

fn block(message: &str) -> AutoModAction {
	let mut block = action(AutoModActionType::BlockMessage);
	block.metadata.custom_message = Some(message.to_owned());
	block
}

fn timeout() -> AutoModAction {
	let mut timeout = action(AutoModActionType::Timeout);
	timeout.metadata.duration_seconds = Some(SEED_TIMEOUT_SECONDS);
	timeout
}

pub fn seed_automod(fixtures: &Fixtures) -> HashMap<u64, Vec<AutoModRule>> {
	let anchor_ms = fixtures.anchor.timestamp_millis();
	fixtures
		.guilds
		.iter()
		.map(|guild| {
			let alert_channel = guild
				.message_channel_ids
				.first()
				.or(guild.channel_ids.first())
				.copied()
				.unwrap_or_default();
			let mut keywords = AutoModRule {
				id: Snowflake(snowflake_at(anchor_ms - SEED_RULE_AGE_MS, 1)),
				name: "No aliens".to_owned(),
				event_type: AutoModEventType::MessageSend,
				trigger_type: AutoModTriggerType::Keyword,
				enabled: true,
				actions: LenientVec::from(vec![
					block("That word is not allowed here."),
					alert(alert_channel),
				]),
				..AutoModRule::default()
			};
			keywords.trigger_metadata.keyword_filter = vec!["alien".to_owned(), "ufo*".to_owned()];
			keywords.trigger_metadata.allow_list = vec!["alienware".to_owned()];
			let mut mentions = AutoModRule {
				id: Snowflake(snowflake_at(anchor_ms - SEED_RULE_AGE_MS, 2)),
				name: "Mention spam".to_owned(),
				event_type: AutoModEventType::MessageSend,
				trigger_type: AutoModTriggerType::MentionSpam,
				enabled: false,
				actions: LenientVec::from(vec![block("Too many mentions."), timeout()]),
				..AutoModRule::default()
			};
			mentions.trigger_metadata.mention_total_limit = Some(SEED_MENTION_LIMIT);
			mentions.trigger_metadata.mention_raid_protection_enabled = true;
			for rule in [&mut keywords, &mut mentions] {
				rule.guild_id = Some(Snowflake(guild.id));
				rule.creator_id = Some(Snowflake(CURRENT_USER_ID));
			}
			(guild.id, vec![keywords, mentions])
		})
		.collect()
}

fn decode(value: Value) -> Result<AutoModRule, ApiError> {
	serde_json::from_value(value).map_err(|error| ApiError::invalid_body(error.to_string()))
}

impl Shared {
	pub fn automod_rules_json(&self, guild_id: u64) -> Result<Value, ApiError> {
		let store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		Ok(to_json(
			store.automod_rules.get(&guild_id).map_or(&[][..], Vec::as_slice),
		))
	}

	pub fn create_automod_rule(&self, guild_id: u64, body: Value) -> Result<Value, ApiError> {
		let mut rule = decode(body)?;
		if rule.name.is_empty() || rule.actions.is_empty() {
			return Err(ApiError::invalid_body("name and actions are required"));
		}
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let id = store.next_snowflake();
		rule.id = Snowflake(id);
		rule.guild_id = Some(Snowflake(guild_id));
		rule.creator_id = Some(Snowflake(CURRENT_USER_ID));
		let value = to_json(&rule);
		let changes = json!([{ "key": "name", "new_value": rule.name }]);
		store.automod_rules.entry(guild_id).or_default().push(rule);
		store.push_audit(guild_id, ACTION_AUTOMOD_RULE_CREATE, Some(id), changes, None);
		Ok(value)
	}

	pub fn modify_automod_rule(&self, guild_id: u64, rule_id: u64, body: &Value) -> Result<Value, ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let slot = store
			.automod_rules
			.get_mut(&guild_id)
			.and_then(|rules| rules.iter_mut().find(|rule| rule.id.0 == rule_id))
			.ok_or_else(ApiError::not_found)?;
		let mut merged = to_json(&*slot);
		for (key, value) in body.as_object().into_iter().flatten() {
			merged[key] = value.clone();
		}
		*slot = decode(merged)?;
		let value = to_json(&*slot);
		let changes = json!([{ "key": "name", "new_value": slot.name }]);
		store.push_audit(guild_id, ACTION_AUTOMOD_RULE_UPDATE, Some(rule_id), changes, None);
		Ok(value)
	}

	pub fn delete_automod_rule(&self, guild_id: u64, rule_id: u64) -> Result<(), ApiError> {
		let mut store = self.store.lock();
		self.live_guild(&store, guild_id)?;
		let rules = store
			.automod_rules
			.get_mut(&guild_id)
			.ok_or_else(ApiError::not_found)?;
		let name = rules
			.iter()
			.find(|rule| rule.id.0 == rule_id)
			.map(|rule| rule.name.clone())
			.ok_or_else(ApiError::not_found)?;
		rules.retain(|rule| rule.id.0 != rule_id);
		store.push_audit(
			guild_id,
			ACTION_AUTOMOD_RULE_DELETE,
			Some(rule_id),
			json!([{ "key": "name", "old_value": name }]),
			None,
		);
		Ok(())
	}
}
