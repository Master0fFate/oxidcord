use std::collections::BTreeSet;

use plugin_sdk::model::{MessageFull, ViewEvent, ViewEventKind};
use plugin_sdk::patch::Patch;
use plugin_sdk::view;
use plugin_sdk::{Md, Plugin, PluginExt, Reply, export_plugin, log, mount, storage};

const HIDDEN_KEY: &str = "hidden";
const MOUNT_KEY: &str = "toggle";
const BUTTON: &str = "toggle";
const MAX_HIDDEN: usize = 2_000;

#[derive(Default)]
struct HideAttachments {
	hidden: BTreeSet<u64>,
}

impl HideAttachments {
	fn toggle(&mut self, message_id: u64) {
		if !self.hidden.remove(&message_id) {
			self.hidden.insert(message_id);
			if self.hidden.len() > MAX_HIDDEN {
				self.hidden.pop_first();
			}
		}
		let ids: Vec<String> = self.hidden.iter().map(u64::to_string).collect();
		if let Err(error) = storage::set(HIDDEN_KEY, ids.join(",").as_bytes()) {
			log::warn!("could not save the hidden messages: {error:?}");
		}
	}
}

impl Plugin for HideAttachments {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(HIDDEN_KEY) {
			self.hidden = String::from_utf8_lossy(&saved)
				.split(',')
				.filter_map(|id| id.parse().ok())
				.collect();
		}
		let button = view::icon_button(BUTTON, "eye-off", "Toggle media");
		let options = mount::Options::new("Toggle media");
		if let Err(error) = mount::mount(mount::MESSAGE_ACTION, MOUNT_KEY, [button], options) {
			log::warn!("could not add the message action: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for HideAttachments {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.node == BUTTON
			&& event.kind == ViewEventKind::Click
			&& let Some(message_id) = event.context.message_id
		{
			self.toggle(message_id);
		}
		Reply::none()
	}

	fn intercept_message(&mut self, msg: &MessageFull, _content: Vec<Md>) -> Patch {
		if !self.hidden.contains(&msg.view.id) || (msg.attachments.is_empty() && msg.embeds.is_empty()) {
			return Patch::keep();
		}
		let patch = msg.attachments.iter().fold(Patch::keep(), |patch, attachment| {
			patch.hide_attachment(attachment.id)
		});
		(0..msg.embeds.len())
			.fold(patch, Patch::hide_embed)
			.annotate("Media hidden")
	}
}

export_plugin!(HideAttachments, ext);
