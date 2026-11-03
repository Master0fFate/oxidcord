use std::sync::Arc;

use protocol::{
	AllowedMentions, CreateMessage, DispatchEvent, Message, MessageFlags, MessageReference, Nonce, Snowflake,
};
use state::LocalEvent;
use tokio::task::AbortHandle;

use rest::{RestError, UploadProgress};

use super::actions::send_failure_reason;
use super::outgoing::Outgoing;
use super::{Core, Internal};

const MIB: u64 = 1024 * 1024;

pub(super) struct Sending {
	pub(super) channel_id: Snowflake,
	pub(super) outgoing: Outgoing,
	pub(super) task: AbortHandle,
}

fn size_label(bytes: u64) -> String {
	if bytes >= 1024 * MIB {
		format!("{} GiB", bytes / (1024 * MIB))
	} else {
		format!("{} MiB", bytes / MIB)
	}
}

fn create_body(nonce: &Nonce, channel_id: Snowflake, outgoing: &Outgoing) -> CreateMessage {
	let reply_to = outgoing.reply_to;
	CreateMessage {
		content: (!outgoing.content.is_empty()).then(|| outgoing.content.clone()),
		nonce: Some(nonce.0.clone()),
		enforce_nonce: Some(true),
		message_reference: reply_to.map(|reply| MessageReference {
			message_id: Some(reply.message),
			channel_id: Some(channel_id),
			fail_if_not_exists: Some(false),
			..MessageReference::default()
		}),
		allowed_mentions: reply_to.filter(|reply| !reply.mention).map(|_| AllowedMentions {
			parse: Some(vec![
				"users".to_owned(),
				"roles".to_owned(),
				"everyone".to_owned(),
			]),
			replied_user: Some(false),
			..AllowedMentions::default()
		}),
		flags: outgoing.silent.then_some(MessageFlags::SUPPRESS_NOTIFICATIONS),
		sticker_ids: outgoing.stickers.clone(),
		..CreateMessage::default()
	}
}

impl Core {
	pub(super) fn within_limits(&mut self, channel_id: Snowflake, outgoing: &Outgoing) -> bool {
		let (upload_limit, message_limit) = {
			let state = self.state.read();
			(state.upload_limit(channel_id), state.message_limit())
		};
		if outgoing.content.chars().count() > message_limit {
			self.reject(format!("Message is over {message_limit} characters"));
			return false;
		}
		let oversized = outgoing
			.files
			.iter()
			.any(|file| file.bytes.len() as u64 > upload_limit);
		if oversized {
			self.reject(format!("Files are limited to {}", size_label(upload_limit)));
		}
		!oversized
	}

	pub(super) fn spawn_send(
		&self,
		nonce: &Nonce,
		channel_id: Snowflake,
		outgoing: &Outgoing,
	) -> AbortHandle {
		let body = create_body(nonce, channel_id, outgoing);
		let files = outgoing.files.clone();
		let nonce = nonce.clone();
		let rest = self.rest.clone();
		let internal = self.internal.clone();
		self.runtime
			.spawn(async move {
				let sent = if files.is_empty() {
					rest.create_message(channel_id, body).await
				} else {
					let progress: UploadProgress = {
						let internal = internal.clone();
						let nonce = nonce.clone();
						Arc::new(move |uploaded| {
							let _ = internal.send(Internal::UploadProgress {
								nonce: nonce.clone(),
								uploaded,
							});
						})
					};
					rest.create_message_with_files(channel_id, body, files, Some(progress))
						.await
				};
				let _ = internal.send(match sent {
					Ok(message) => Internal::Sent {
						nonce,
						message: Box::new(message),
					},
					Err(error) => {
						tracing::warn!(%error, "message was not sent");
						if let RestError::Slowmode { retry_after } = &error {
							let _ = internal.send(Internal::SlowmodeHit {
								channel_id,
								retry_after: *retry_after,
							});
						}
						Internal::SendFailed {
							nonce,
							reason: send_failure_reason(&error),
						}
					}
				});
			})
			.abort_handle()
	}

	pub(super) fn on_sent(&mut self, nonce: &Nonce, message: Box<Message>) {
		self.sending.remove(nonce);
		let waiting = self
			.state
			.read()
			.messages(message.channel_id)
			.is_some_and(|cache| cache.has_pending(nonce));
		if !waiting {
			return;
		}
		let (channel_id, id) = (message.channel_id, message.id);
		self.apply_dispatch(&DispatchEvent::MessageCreate(message));
		self.persist_cached_message(channel_id, id);
		self.persist_read_state(channel_id);
	}

	pub(super) fn on_send_failed(&mut self, nonce: Nonce, reason: String) {
		let Some(sending) = self.sending.get(&nonce) else {
			return;
		};
		let channel_id = sending.channel_id;
		if !sending.outgoing.shows_pending() {
			self.sending.remove(&nonce);
			self.reject(reason);
			return;
		}
		let waiting = self
			.state
			.read()
			.messages(channel_id)
			.is_some_and(|cache| cache.has_pending(&nonce));
		if waiting {
			self.apply_local(LocalEvent::PendingFailed {
				nonce,
				reason: Some(reason),
				retryable: true,
			});
		} else {
			self.sending.remove(&nonce);
		}
	}

	pub(super) fn on_upload_progress(&mut self, nonce: Nonce, uploaded: u64) {
		if self.sending.contains_key(&nonce) {
			self.apply_local(LocalEvent::UploadProgress { nonce, uploaded });
		}
	}

	pub(super) fn retry_pending(&mut self, nonce: Nonce) {
		let Some(channel_id) = self.sending.get(&nonce).map(|sending| sending.channel_id) else {
			return;
		};
		if !self.send_allowed(channel_id) {
			return;
		}
		let Some(sending) = self.sending.remove(&nonce) else {
			return;
		};
		self.apply_local(LocalEvent::PendingRetried { nonce: nonce.clone() });
		self.dispatch_send(nonce, channel_id, sending.outgoing);
	}

	pub(super) fn prune_sending(&mut self) {
		let state = self.state.read();
		self.sending.retain(|nonce, sending| {
			!sending.task.is_finished()
				|| state
					.messages(sending.channel_id)
					.is_some_and(|cache| cache.has_pending(nonce))
		});
	}

	pub(super) fn discard_pending(&mut self, nonce: Nonce) {
		if let Some(sending) = self.sending.remove(&nonce) {
			sending.task.abort();
		}
		self.apply_local(LocalEvent::PendingDiscarded { nonce });
	}
}
