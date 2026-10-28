use plugin_host::api::model::{OutgoingAttachment, OutgoingFull};
use protocol::Snowflake;
use rest::UploadFile;

use crate::command::ReplyTarget;

const SPOILER_PREFIX: &str = "SPOILER_";

pub(crate) struct Outgoing {
	pub(crate) content: String,
	pub(crate) reply_to: Option<ReplyTarget>,
	pub(crate) files: Vec<UploadFile>,
	pub(crate) stickers: Vec<Snowflake>,
	pub(crate) silent: bool,
}

fn plain_name(name: &str) -> &str {
	name.strip_prefix(SPOILER_PREFIX).unwrap_or(name)
}

impl Outgoing {
	pub(crate) fn plain(content: String) -> Self {
		Self {
			content,
			reply_to: None,
			files: Vec::new(),
			stickers: Vec::new(),
			silent: false,
		}
	}

	pub(crate) fn has_payload(&self) -> bool {
		!self.content.is_empty() || !self.files.is_empty() || !self.stickers.is_empty()
	}

	pub(crate) fn shows_pending(&self) -> bool {
		!self.content.is_empty() || !self.files.is_empty()
	}

	pub(crate) fn to_full(&self, channel_id: Snowflake) -> OutgoingFull {
		OutgoingFull {
			channel_id: channel_id.get(),
			content: self.content.clone(),
			reply_to: self.reply_to.map(|reply| reply.message.get()),
			mention_reply: self.reply_to.is_some_and(|reply| reply.mention),
			silent: self.silent,
			attachments: self
				.files
				.iter()
				.enumerate()
				.map(|(handle, file)| OutgoingAttachment {
					handle: handle as u64,
					name: plain_name(&file.name).to_owned(),
					size: file.bytes.len() as u64,
					content_type: Some(file.content_type.clone()).filter(|kind| !kind.is_empty()),
					spoiler: file.name.starts_with(SPOILER_PREFIX),
				})
				.collect(),
			stickers: self.stickers.iter().map(|sticker| sticker.get()).collect(),
		}
	}

	pub(crate) fn rewritten(self, message: OutgoingFull, usable: impl Fn(Snowflake) -> bool) -> Self {
		let mut files: Vec<Option<UploadFile>> = self.files.into_iter().map(Some).collect();
		let mut kept = Vec::with_capacity(files.len());
		for attachment in message.attachments {
			let Some(mut file) = usize::try_from(attachment.handle)
				.ok()
				.and_then(|index| files.get_mut(index))
				.and_then(Option::take)
			else {
				continue;
			};
			file.name = if attachment.spoiler {
				format!("{SPOILER_PREFIX}{}", plain_name(&attachment.name))
			} else {
				plain_name(&attachment.name).to_owned()
			};
			kept.push(file);
		}
		Self {
			content: message.content,
			reply_to: self.reply_to.map(|reply| ReplyTarget {
				mention: message.mention_reply,
				..reply
			}),
			files: kept,
			stickers: message
				.stickers
				.into_iter()
				.map(Snowflake)
				.filter(|&sticker| usable(sticker))
				.collect(),
			silent: message.silent,
		}
	}
}
