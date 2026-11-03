use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use protocol::EditMessage;
use serde_json::{Value, json};

use crate::json::parse_id;
use crate::ops::Draft;
use crate::state::Shared;

const CLOSE_SESSION_TIMED_OUT: u16 = 4009;
const AWAY_CHANNEL_INDEX: usize = 1;

impl Shared {
	fn post_while_away(&self) -> Option<u64> {
		let guild = self.fixtures.guilds.first()?;
		let channel_id = *guild
			.message_channel_ids
			.get(AWAY_CHANNEL_INDEX)
			.or(guild.message_channel_ids.first())?;
		let author = guild.members.get(1)?.user.clone()?;
		let newest = self.store.lock().newest_message_id(channel_id);
		let first = self
			.post_message(channel_id, &author, Draft::text("sent while you were away"))
			.ok()
			.and_then(|message| parse_id(&message["id"]))?;
		self.post_message(channel_id, &author, Draft::text("also sent while you were away"))
			.ok()?;
		let edit = EditMessage {
			content: Some("edited while you were away".to_owned()),
			..EditMessage::default()
		};
		self.edit_as(channel_id, first, author.id.0, edit).ok()?;
		if let Some(newest) = newest {
			self.delete_message(channel_id, newest).ok()?;
		}
		Some(channel_id)
	}

	pub fn drop_sessions(&self) -> Value {
		let _ = self.incoming_call();
		self.hub.expire_all(CLOSE_SESSION_TIMED_OUT);
		json!({ "channel_id": self.post_while_away().map(|id| id.to_string()) })
	}
}

pub async fn reconnect(State(shared): State<Arc<Shared>>) -> Json<Value> {
	Json(shared.drop_sessions())
}
