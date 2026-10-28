use plugin_sdk::model::{ClientOption, UiContext};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{
	Event, HostError, MessageRef, Plugin, PluginExt, actions, export_plugin, log, menu, mount, storage, ui,
};

const BY_DEFAULT_KEY: &str = "superReactByDefault";
const MENU_ITEM: &str = "super";
const TOAST_MS: u32 = 3000;

struct SuperReactionTweaks {
	by_default: bool,
}

impl Default for SuperReactionTweaks {
	fn default() -> Self {
		Self { by_default: true }
	}
}

fn describe(error: &HostError) -> String {
	match error {
		HostError::Denied(_) => "A permission is missing".to_owned(),
		HostError::RateLimited => "Too many reactions, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "Discord took too long".to_owned(),
		HostError::Unavailable => "Not available right now".to_owned(),
	}
}

fn notice(text: &str) {
	if let Err(error) = mount::toast([view::text(text)], TOAST_MS) {
		log::debug!("could not show the toast: {error:?}");
	}
}

impl SuperReactionTweaks {
	fn apply_default(&self) {
		if let Err(error) = actions::set_client_option(ClientOption::SuperReactDefault(self.by_default)) {
			log::warn!("could not change the reaction default: {error:?}");
		}
	}
}

impl Plugin for SuperReactionTweaks {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{BY_DEFAULT_KEY}")) {
			self.by_default = !matches!(String::from_utf8_lossy(&saved).trim(), "false" | "0" | "off");
		}
		self.apply_default();
		if let Err(error) = menu::set(menu::REACTION, [menu::item(MENU_ITEM, "Super react")]) {
			log::warn!("could not add the menu entry: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == BY_DEFAULT_KEY
		{
			self.by_default = !matches!(value.trim(), "false" | "0" | "off");
			self.apply_default();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"superReactByDefault":{"type":"boolean","title":"Super react by default","description":"Reaction picker will default to Super Reactions","default":true},"unlimitedSuperReactionPlaying":{"type":"boolean","title":"Unlimited playing","description":"Not applicable, the client has no limit on playing super reactions","default":false},"superReactionPlayingLimit":{"type":"integer","title":"Playing limit","description":"Not applicable, the client has no limit on playing super reactions","minimum":0,"maximum":100,"default":20}}}"#
			.to_owned()
	}
}

impl PluginExt for SuperReactionTweaks {
	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		if menu != menu::REACTION || key != MENU_ITEM {
			return;
		}
		let (Some(id), Some(channel_id), Some(emoji)) = (
			context.message_id,
			context.channel_id,
			ui::context_detail().and_then(|detail| detail.reaction),
		) else {
			return;
		};
		let message = MessageRef {
			id,
			channel_id,
			guild_id: context.guild_id,
		};
		if let Err(error) = actions::react(message, &emoji, true) {
			notice(&describe(&error));
		}
	}
}

export_plugin!(SuperReactionTweaks, ext);
