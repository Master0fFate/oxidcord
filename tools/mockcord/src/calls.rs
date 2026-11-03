use serde_json::{Value, json};

use crate::error::ApiError;
use crate::fixtures::CURRENT_USER_ID;
use crate::json::to_json;
use crate::state::Shared;

const GUEST_SESSION: &str = "mock-guest";
const GUEST_MEMBER_INDEX: usize = 1;

fn ids(users: &[u64]) -> Vec<String> {
	users.iter().map(u64::to_string).collect()
}

impl Shared {
	pub fn ring_call(&self, channel_id: u64, recipients: Option<Vec<u64>>) -> Result<(), ApiError> {
		let (created, ringing) = {
			let store = &mut *self.store.lock();
			let channel = store
				.channels
				.get(&channel_id)
				.ok_or_else(ApiError::unknown_channel)?;
			let members: Vec<u64> = channel.recipient_ids.iter().map(|id| id.0).collect();
			let targets = recipients.unwrap_or_else(|| members.clone());
			let created = !store.calls.contains_key(&channel_id);
			let ringing = store.calls.entry(channel_id).or_default();
			for user in targets.into_iter().filter(|user| members.contains(user)) {
				if !ringing.contains(&user) {
					ringing.push(user);
				}
			}
			(created, ringing.clone())
		};
		self.broadcast_call(
			if created { "CALL_CREATE" } else { "CALL_UPDATE" },
			channel_id,
			&ringing,
		);
		Ok(())
	}

	pub fn stop_ringing(&self, channel_id: u64, recipients: Option<Vec<u64>>) -> Result<(), ApiError> {
		let in_call = self
			.voice_join
			.lock()
			.as_ref()
			.is_some_and(|join| join.channel_id == channel_id);
		let remaining = {
			let mut store = self.store.lock();
			if !store.channels.contains_key(&channel_id) {
				return Err(ApiError::unknown_channel());
			}
			let Some(ringing) = store.calls.get_mut(&channel_id) else {
				return Ok(());
			};
			match &recipients {
				Some(users) => ringing.retain(|user| !users.contains(user)),
				None => ringing.clear(),
			}
			let remaining = ringing.clone();
			if remaining.is_empty() && !in_call {
				store.calls.remove(&channel_id);
			}
			remaining
		};
		let kind = if remaining.is_empty() && !in_call {
			"CALL_DELETE"
		} else {
			"CALL_UPDATE"
		};
		self.broadcast_call(kind, channel_id, &remaining);
		Ok(())
	}

	pub fn incoming_call(&self) -> Result<(), ApiError> {
		let channel_id = self
			.store
			.lock()
			.private_channel_ids
			.first()
			.copied()
			.ok_or_else(ApiError::unknown_channel)?;
		self.store.lock().calls.insert(channel_id, vec![CURRENT_USER_ID]);
		self.broadcast_call("CALL_CREATE", channel_id, &[CURRENT_USER_ID]);
		Ok(())
	}

	pub fn guest_voice(&self, joined: bool) -> Result<(), ApiError> {
		let join = self
			.voice_join
			.lock()
			.clone()
			.ok_or(ApiError::invalid_body("Not in a voice channel"))?;
		let (guest, member) = self.guest_of(join.guild_id, join.channel_id)?;
		let event = json!({
			"guild_id": join.guild_id.map(|id| id.to_string()),
			"channel_id": joined.then(|| join.channel_id.to_string()),
			"user_id": guest.to_string(),
			"member": member,
			"session_id": GUEST_SESSION,
			"deaf": false,
			"mute": false,
			"self_deaf": false,
			"self_mute": false,
			"self_stream": false,
			"self_video": false,
			"suppress": false,
			"request_to_speak_timestamp": null,
		});
		self.hub.broadcast("VOICE_STATE_UPDATE", &event);
		if join.guild_id.is_none() && joined {
			self.stop_ringing(join.channel_id, Some(vec![guest]))?;
		}
		Ok(())
	}

	fn guest_of(&self, guild_id: Option<u64>, channel_id: u64) -> Result<(u64, Value), ApiError> {
		match guild_id {
			Some(guild_id) => {
				let member = self
					.fixtures
					.guild(guild_id)
					.and_then(|guild| guild.members.get(GUEST_MEMBER_INDEX))
					.ok_or_else(ApiError::not_found)?;
				let user = member.user.as_ref().ok_or_else(ApiError::not_found)?;
				Ok((user.id.0, to_json(member)))
			}
			None => {
				let store = self.store.lock();
				let guest = store
					.channels
					.get(&channel_id)
					.and_then(|channel| channel.recipient_ids.iter().find(|id| id.0 != CURRENT_USER_ID))
					.ok_or_else(ApiError::unknown_channel)?;
				Ok((guest.0, Value::Null))
			}
		}
	}

	fn broadcast_call(&self, kind: &str, channel_id: u64, ringing: &[u64]) {
		self.hub.broadcast(
			kind,
			&json!({
				"channel_id": channel_id.to_string(),
				"ringing": ids(ringing),
				"region": "mock",
			}),
		);
	}
}
