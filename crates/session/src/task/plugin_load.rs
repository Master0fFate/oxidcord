use std::sync::Arc;

use plugin_host::api::HostError;
use plugin_host::api::User as PluginUser;
use plugin_host::api::model::UserProfile;
use plugin_host::convert::user_view;
use protocol::{MessagesQuery, Snowflake};
use rest::RestError;
use state::Loaded;
use tokio::sync::mpsc;

use super::{Core, Internal};
use crate::broker::fetched_full;
use crate::command::{LoadResult, PluginLoad, Reply};
use crate::views;

const REACTION_PAGE: u8 = 100;

pub(crate) enum PluginAnswer {
	Profile {
		user_id: Snowflake,
		reply: Reply<LoadResult<Option<UserProfile>>>,
	},
	User {
		user_id: Snowflake,
		reply: Reply<LoadResult<Option<PluginUser>>>,
	},
	Reactions {
		message_id: Snowflake,
		emoji: String,
		reply: Reply<LoadResult<Vec<PluginUser>>>,
	},
}

fn answer_error<T>(error: &RestError, absent: T) -> LoadResult<T> {
	match error {
		RestError::NotFound | RestError::Forbidden => Ok(absent),
		RestError::RateLimited { .. } => Err(HostError::RateLimited),
		other => {
			tracing::debug!(error = %other, "plugin load failed");
			Err(HostError::Unavailable)
		}
	}
}

fn media_error(error: &media::MediaError) -> HostError {
	match error {
		media::MediaError::DisallowedHost(_) => HostError::Invalid("host is not allowed".to_owned()),
		media::MediaError::Limit(_) => HostError::Invalid("response is too large".to_owned()),
		other => {
			tracing::debug!(error = %other, "plugin media load failed");
			HostError::Unavailable
		}
	}
}

fn apply_then_answer(internal: &mpsc::UnboundedSender<Internal>, loaded: Loaded, answer: PluginAnswer) {
	let _ = internal.send(Internal::PluginApplied { loaded, answer });
}

impl Core {
	pub(super) fn on_plugin_load(&mut self, load: PluginLoad) {
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		match load {
			PluginLoad::Profile { user_id, reply } => {
				self.runtime.spawn(async move {
					match rest.user_profile(user_id, None).await {
						Ok(raw) => apply_then_answer(
							&internal,
							Loaded::Profile { user_id, raw },
							PluginAnswer::Profile { user_id, reply },
						),
						Err(error) => reply.send(answer_error(&error, None)),
					}
				});
			}
			PluginLoad::User { user_id, reply } => {
				self.runtime.spawn(async move {
					match rest.user(user_id).await {
						Ok(user) => apply_then_answer(
							&internal,
							Loaded::User(Box::new(user)),
							PluginAnswer::User { user_id, reply },
						),
						Err(error) => reply.send(answer_error(&error, None)),
					}
				});
			}
			PluginLoad::ReactionUsers {
				channel_id,
				message_id,
				emoji,
				reply,
			} => {
				self.runtime.spawn(async move {
					match rest
						.reactions(channel_id, message_id, &emoji, REACTION_PAGE)
						.await
					{
						Ok(users) => apply_then_answer(
							&internal,
							Loaded::ReactionUsers {
								message_id,
								emoji: emoji.clone(),
								users: users.into_vec(),
							},
							PluginAnswer::Reactions {
								message_id,
								emoji,
								reply,
							},
						),
						Err(error) => reply.send(answer_error(&error, Vec::new())),
					}
				});
			}
			PluginLoad::Message {
				channel_id,
				message_id,
				reply,
			} => self.load_plugin_message(channel_id, message_id, reply),
			PluginLoad::Media { url, reply } => {
				let Some(fetcher) = self.media.clone() else {
					reply.send(Err(HostError::Unavailable));
					return;
				};
				self.runtime.spawn(async move {
					let fetched = fetcher.fetch(&url).await;
					reply.send(
						fetched
							.map(|bytes| bytes.to_vec())
							.map_err(|error| media_error(&error)),
					);
				});
			}
		}
	}

	fn load_plugin_message(
		&self,
		channel_id: Snowflake,
		message_id: Snowflake,
		reply: Reply<LoadResult<Option<plugin_host::api::model::MessageFull>>>,
	) {
		let rest = self.rest.clone();
		let state = self.state.clone();
		let loads = Arc::clone(&self.loads);
		self.runtime.spawn(async move {
			let query = MessagesQuery {
				around: Some(message_id),
				before: None,
				after: None,
				limit: Some(1),
			};
			match rest.messages(channel_id, query).await {
				Ok(messages) => {
					let found = messages
						.into_vec()
						.into_iter()
						.find(|message| message.id == message_id);
					let full = found.map(|message| {
						let message = Arc::new(message);
						loads.remember_message(Arc::clone(&message));
						fetched_full(&state.read(), &message)
					});
					reply.send(Ok(full));
				}
				Err(error) => reply.send(answer_error(&error, None)),
			}
		});
	}

	pub(super) fn on_plugin_applied(&mut self, loaded: Loaded, answer: PluginAnswer) {
		self.on_loaded(loaded);
		let state = self.state.read();
		match answer {
			PluginAnswer::Profile { user_id, reply } => {
				reply.send(Ok(state.profile(user_id).map(views::user_profile)));
			}
			PluginAnswer::User { user_id, reply } => {
				reply.send(Ok(state.user(user_id).map(|user| user_view(user))));
			}
			PluginAnswer::Reactions {
				message_id,
				emoji,
				reply,
			} => {
				let users = state
					.reaction_users(message_id, &emoji)
					.map(|users| users.iter().map(user_view).collect())
					.unwrap_or_default();
				reply.send(Ok(users));
			}
		}
	}
}
