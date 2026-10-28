use std::collections::HashSet;

use plugin_sdk::model::{
	ClickKind, ClickTarget, ClickWatch, MessageClick, MessageFull, MessageMeta, Modifiers,
};
use plugin_sdk::patch::PatchExt;
use plugin_sdk::{Md, MessageRef, Plugin, PluginExt, actions, client, export_plugin, log, ui};

const REVEALED_CAPACITY: usize = 2000;
const CHANNEL_WINDOW: u32 = 100;
const MAX_REFRESHES: usize = 40;
const SPOILER_FILE_PREFIX: &str = "SPOILER_";

#[derive(Default)]
struct RevealAllSpoilers {
	revealed: HashSet<u64>,
}

fn has_spoilers(msg: &MessageFull) -> bool {
	msg.view.content.contains("||")
		|| msg
			.attachments
			.iter()
			.any(|attachment| attachment.filename.starts_with(SPOILER_FILE_PREFIX))
}

impl RevealAllSpoilers {
	fn reveal(&mut self, message: MessageRef) {
		if self.revealed.len() >= REVEALED_CAPACITY {
			self.revealed.clear();
		}
		self.revealed.insert(message.id);
		if let Err(error) = actions::invalidate_message(message) {
			log::debug!("could not refresh message {}: {error:?}", message.id);
		}
	}

	fn reveal_channel(&mut self, click: &MessageClick) {
		let messages = match client::channel_messages(click.message.channel_id, CHANNEL_WINDOW) {
			Ok(messages) => messages,
			Err(error) => {
				log::debug!("could not read the channel: {error:?}");
				return;
			}
		};
		let spoilers = messages
			.iter()
			.rev()
			.filter(|message| has_spoilers(message))
			.take(MAX_REFRESHES);
		for message in spoilers {
			self.reveal(MessageRef {
				id: message.view.id,
				channel_id: message.view.channel_id,
				guild_id: message.view.guild_id,
			});
		}
	}
}

impl Plugin for RevealAllSpoilers {
	fn init(&mut self) -> Result<(), String> {
		for modifiers in [Modifiers::CTRL, Modifiers::CTRL | Modifiers::SHIFT] {
			let watch = ClickWatch {
				kinds: vec![ClickKind::Single],
				modifiers,
				targets: vec![ClickTarget::Spoiler(0)],
			};
			if let Err(error) = ui::watch_clicks(&watch) {
				log::warn!("could not watch spoiler clicks: {error:?}");
			}
		}
		Ok(())
	}
}

impl PluginExt for RevealAllSpoilers {
	fn on_message_click(&mut self, click: MessageClick) {
		if click.modifiers.contains(Modifiers::SHIFT) {
			self.reveal_channel(&click);
		} else {
			self.reveal(click.message);
		}
	}

	fn intercept_message_ext(
		&mut self,
		msg: &MessageFull,
		_meta: &MessageMeta,
		_content: Vec<Md>,
	) -> PatchExt {
		if self.revealed.contains(&msg.view.id) {
			PatchExt::keep().reveal_spoilers()
		} else {
			PatchExt::keep()
		}
	}
}

export_plugin!(RevealAllSpoilers, ext);
