use activity::{ActivityRequest, SecretKind};
use protocol::{CreateMessage, Message, MessageActivity, Snowflake};

use super::Core;
use super::Internal;
use super::actions::rest_failure;

impl Core {
	pub(super) fn join_activity(
		&self,
		user_id: Snowflake,
		session_id: String,
		application_id: Snowflake,
		kind: SecretKind,
		invite: Option<(Snowflake, Snowflake)>,
	) {
		let rest = self.rest.clone();
		let activity = self.activity.clone();
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			let failure = match rest
				.activity_secret(user_id, &session_id, application_id, kind.action_type(), invite)
				.await
			{
				Ok(secret) if activity.deliver_secret(application_id, kind, &secret) => return,
				Ok(_) => Internal::Rejected("join: the game is not running".to_owned()),
				Err(error) => rest_failure("join", &error),
			};
			let _ = internal.send(failure);
		});
	}

	pub(super) fn ask_to_join(&self, user_id: Snowflake, application_id: Snowflake, party_id: String) {
		self.send_activity_message(
			"ask to join",
			user_id,
			application_id,
			MessageActivity::JOIN_REQUEST,
			party_id,
		);
	}

	pub(super) fn on_activity_request(&self, request: ActivityRequest) {
		match request {
			ActivityRequest::JoinInvite {
				application_id,
				user_id,
				party_id,
			} => {
				self.send_activity_message("invite", user_id, application_id, MessageActivity::JOIN, party_id)
			}
		}
	}

	pub(super) fn deliver_join_request(&self, message: &Message) {
		let Some(activity) = &message.activity else {
			return;
		};
		let application_id = message
			.application_id
			.or_else(|| message.application.as_ref().map(|application| application.id));
		let Some(application_id) = application_id.filter(|_| {
			activity.kind == Some(MessageActivity::JOIN_REQUEST)
				&& self.state.read().current_user_id() != Some(message.author.id)
		}) else {
			return;
		};
		self.activity
			.deliver_join_request(application_id, &message.author);
	}

	fn send_activity_message(
		&self,
		what: &'static str,
		user_id: Snowflake,
		application_id: Snowflake,
		kind: i64,
		party_id: String,
	) {
		let body = CreateMessage {
			activity: Some(MessageActivity {
				kind: Some(kind),
				party_id: Some(party_id),
				session_id: Some(self.state.read().session().session_id.clone()),
			}),
			application_id: Some(application_id),
			..CreateMessage::default()
		};
		self.spawn_action(what, move |rest| async move {
			let channel = rest.create_dm(user_id).await?;
			rest.create_message(channel.id, body).await.map(drop)
		});
	}
}
