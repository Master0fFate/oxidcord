use std::future::Future;

use chrono::{DateTime, Utc};
use plugin_host::api::model::NotificationSettings;
use protocol::{ChannelType, Permissions, Snowflake};
use rest::{RestClient, RestError};
use serde_json::{Map, Value, json};
use state::{Loaded, LocalEvent};

use super::{Core, Internal};

pub(super) fn object<const N: usize>(fields: [(&str, Option<Value>); N]) -> Value {
	Value::Object(
		fields
			.into_iter()
			.filter_map(|(key, value)| Some((key.to_owned(), value?)))
			.collect::<Map<String, Value>>(),
	)
}

pub(super) fn some<T: Into<Value>>(value: Option<T>) -> Option<Value> {
	value.map(Into::into)
}

pub(super) fn timestamp(time: DateTime<Utc>) -> String {
	time.to_rfc3339()
}

fn failure_reason(what: &str, error: &RestError) -> String {
	match error {
		RestError::Forbidden => format!("{what}: missing permissions"),
		RestError::NotFound => format!("{what}: not found"),
		RestError::RateLimited { .. } => format!("{what}: rate limited, try again shortly"),
		RestError::Http(_, Some(api)) if !api.message.is_empty() => format!("{what}: {}", api.message),
		_ => format!("{what} failed"),
	}
}

impl Core {
	pub(super) fn reject(&mut self, reason: impl Into<String>) {
		self.apply_local(LocalEvent::CommandRejected {
			reason: reason.into(),
		});
	}

	pub(super) fn on_loaded(&mut self, loaded: Loaded) {
		self.apply_local(LocalEvent::Loaded(Box::new(loaded)));
	}

	pub(super) fn guild_allows(&mut self, guild_id: Snowflake, needed: Permissions, what: &str) -> bool {
		let allowed = {
			let state = self.state.read();
			state
				.current_user_id()
				.is_some_and(|me| state.guild_permissions(guild_id, me).intersects(needed))
		};
		if !allowed {
			self.reject(format!("You need the {what} permission"));
		}
		allowed
	}

	pub(super) fn channel_allows(&mut self, channel_id: Snowflake, needed: Permissions, what: &str) -> bool {
		let allowed = {
			let state = self.state.read();
			state.channel_guild(channel_id).is_none() || state.my_permissions(channel_id).contains(needed)
		};
		if !allowed {
			self.reject(format!("You need the {what} permission"));
		}
		allowed
	}

	pub(super) fn may_send(&self, channel_id: Snowflake) -> bool {
		let state = self.state.read();
		state.channel_guild(channel_id).is_none() || state.can_send(channel_id)
	}

	pub(super) fn send_allowed(&mut self, channel_id: Snowflake) -> bool {
		let allowed = self.may_send(channel_id);
		if !allowed {
			self.reject("You can't send messages in this channel");
		}
		allowed
	}

	pub(super) fn channel_guild_allows(
		&mut self,
		channel_id: Snowflake,
		needed: Permissions,
		what: &str,
	) -> Option<Snowflake> {
		let guild_id = self.state.read().channel_guild(channel_id);
		match guild_id {
			Some(guild_id) => self.guild_allows(guild_id, needed, what).then_some(guild_id),
			None => {
				self.reject("This only works in a server");
				None
			}
		}
	}

	pub(super) fn spawn_action<F, Fut>(&self, what: &'static str, call: F)
	where
		F: FnOnce(RestClient) -> Fut,
		Fut: Future<Output = rest::Result<()>> + Send + 'static,
	{
		let request = call(self.rest.clone());
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			if let Err(error) = request.await {
				tracing::warn!(%error, what, "rest call failed");
				let _ = internal.send(Internal::Rejected(failure_reason(what, &error)));
			}
		});
	}

	pub(super) fn spawn_load<F, Fut>(&self, what: &'static str, call: F)
	where
		F: FnOnce(RestClient) -> Fut,
		Fut: Future<Output = rest::Result<Loaded>> + Send + 'static,
	{
		let request = call(self.rest.clone());
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			match request.await {
				Ok(loaded) => {
					let _ = internal.send(Internal::Loaded(loaded));
				}
				Err(error) => {
					tracing::warn!(%error, what, "rest call failed");
					let _ = internal.send(Internal::Rejected(failure_reason(what, &error)));
				}
			}
		});
	}

	pub(super) fn spawn_open<F, Fut>(&self, what: &'static str, call: F)
	where
		F: FnOnce(RestClient) -> Fut,
		Fut: Future<Output = rest::Result<protocol::Channel>> + Send + 'static,
	{
		let request = call(self.rest.clone());
		let internal = self.internal.clone();
		self.runtime.spawn(async move {
			match request.await {
				Ok(channel) => {
					let message = if channel.kind == ChannelType::GroupDm || channel.kind == ChannelType::Dm {
						Internal::DmOpened(Box::new(channel))
					} else {
						Internal::ThreadOpened(Box::new(channel))
					};
					let _ = internal.send(message);
				}
				Err(error) => {
					tracing::warn!(%error, what, "rest call failed");
					let _ = internal.send(Internal::Rejected(failure_reason(what, &error)));
				}
			}
		});
	}

	pub(super) fn on_thread_opened(&mut self, thread: Box<protocol::Channel>) {
		let channel_id = thread.id;
		let guild_id = thread.guild_id;
		self.apply_dispatch(&protocol::DispatchEvent::ThreadCreate(thread));
		self.select_channel(guild_id, channel_id);
	}

	pub(super) fn patch_channel_override(
		&self,
		channel_id: Snowflake,
		guild_id: Option<Snowflake>,
		fields: Value,
	) {
		let mut entry = fields;
		entry["channel_id"] = json!(channel_id);
		let body = json!({ "channel_overrides": { channel_id.to_string(): entry } });
		self.spawn_action("notification settings", move |rest| async move {
			rest.patch_guild_settings(guild_id, body).await.map(drop)
		});
	}

	pub(super) fn patch_thread_settings(&self, thread_id: Snowflake, settings: Value) {
		self.spawn_load("thread settings", move |rest| async move {
			let mut member = rest.update_thread_settings(thread_id, settings).await?;
			member.id = Some(thread_id);
			Ok(Loaded::ThreadMember(Box::new(member)))
		});
	}

	pub(super) fn patch_guild_mute(
		&self,
		guild_id: Option<Snowflake>,
		muted: bool,
		until: Option<DateTime<Utc>>,
	) {
		let body = json!({ "muted": muted, "mute_config": mute_config(muted, until) });
		self.spawn_action("notification settings", move |rest| async move {
			rest.patch_guild_settings(guild_id, body).await.map(drop)
		});
	}
}

fn mute_fields(settings: &NotificationSettings) -> [(&'static str, Option<Value>); 2] {
	let until = settings.mute_until_ms.and_then(DateTime::from_timestamp_millis);
	let muted = settings.muted.or_else(|| until.map(|_| true));
	[
		("muted", muted.map(Value::from)),
		("mute_config", muted.map(|muted| mute_config(muted, until))),
	]
}

fn non_empty(fields: Value) -> Option<Value> {
	fields
		.as_object()
		.is_some_and(|fields| !fields.is_empty())
		.then_some(fields)
}

pub(super) fn guild_notification_body(settings: &NotificationSettings) -> Option<Value> {
	let [muted, mute_config] = mute_fields(settings);
	non_empty(object([
		(
			"message_notifications",
			settings.level.filter(|level| *level <= 2).map(Value::from),
		),
		("suppress_everyone", settings.suppress_everyone.map(Value::from)),
		("suppress_roles", settings.suppress_roles.map(Value::from)),
		("mobile_push", settings.mobile_push.map(Value::from)),
		("hide_muted_channels", settings.hide_muted.map(Value::from)),
		muted,
		mute_config,
	]))
}

pub(super) fn channel_notification_body(
	channel_id: Snowflake,
	settings: &NotificationSettings,
) -> Option<Value> {
	let [muted, mute_config] = mute_fields(settings);
	let mut entry = non_empty(object([
		("message_notifications", settings.level.map(Value::from)),
		muted,
		mute_config,
	]))?;
	entry["channel_id"] = json!(channel_id);
	Some(json!({ "channel_overrides": { channel_id.to_string(): entry } }))
}

pub(super) fn mute_config(muted: bool, until: Option<DateTime<Utc>>) -> Value {
	if !muted {
		return Value::Null;
	}
	match until {
		Some(until) => {
			json!({ "end_time": timestamp(until), "selected_time_window": (until - Utc::now()).num_seconds().max(0) })
		}
		None => json!({ "end_time": null, "selected_time_window": -1 }),
	}
}
