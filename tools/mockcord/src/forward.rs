use protocol::{Message, MessageSnapshot, PartialMessage};

pub fn snapshot_of(source: Message) -> MessageSnapshot {
	MessageSnapshot {
		message: PartialMessage {
			kind: source.kind,
			content: source.content,
			timestamp: source.timestamp,
			edited_timestamp: source.edited_timestamp,
			mentions: source.mentions,
			mention_roles: source.mention_roles,
			attachments: source.attachments,
			embeds: source.embeds,
			sticker_items: source.sticker_items,
			flags: source.flags,
			components: source.components,
		},
	}
}
