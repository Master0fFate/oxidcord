use plugin_sdk::model::{ClickKind, ClickTarget, ClickWatch, MessageClick, Modifiers};
use plugin_sdk::types::SettingChange;
use plugin_sdk::{Event, Plugin, PluginExt, actions, client, export_plugin, log, storage, ui};

const SETTING_KEYS: [&str; 4] = [
	"enableDeleteOnClick",
	"enableDoubleClickToEdit",
	"enableDoubleClickToReply",
	"requireModifier",
];

struct MessageClickActions {
	delete_on_click: bool,
	double_click_edit: bool,
	double_click_reply: bool,
	require_modifier: bool,
}

impl Default for MessageClickActions {
	fn default() -> Self {
		Self {
			delete_on_click: false,
			double_click_edit: true,
			double_click_reply: true,
			require_modifier: false,
		}
	}
}

fn parse_flag(raw: &str) -> bool {
	!matches!(raw.trim(), "false" | "0" | "off")
}

impl MessageClickActions {
	fn set(&mut self, key: &str, value: &str) {
		let on = parse_flag(value);
		match key {
			"enableDeleteOnClick" => self.delete_on_click = on,
			"enableDoubleClickToEdit" => self.double_click_edit = on,
			"enableDoubleClickToReply" => self.double_click_reply = on,
			"requireModifier" => self.require_modifier = on,
			_ => {}
		}
	}

	fn watch(&self) {
		let double = self.double_click_edit || self.double_click_reply;
		let watches = [
			(
				Modifiers::empty(),
				ClickKind::Double,
				double && !self.require_modifier,
			),
			(Modifiers::SHIFT, ClickKind::Double, double),
			(
				Modifiers::CTRL,
				ClickKind::Double,
				double && self.require_modifier,
			),
			(
				Modifiers::CTRL | Modifiers::ALT,
				ClickKind::Single,
				self.delete_on_click,
			),
		];
		for (modifiers, kind, wanted) in watches {
			let watch = ClickWatch {
				kinds: if wanted { vec![kind] } else { Vec::new() },
				modifiers,
				targets: vec![ClickTarget::Body],
			};
			if let Err(error) = ui::watch_clicks(&watch) {
				log::warn!("could not watch message clicks: {error:?}");
			}
		}
	}

	fn double_click(&self, click: &MessageClick) {
		let Some(message) = client::get_message(click.message.channel_id, click.message.id)
			.ok()
			.flatten()
		else {
			return;
		};
		let outcome = if message.mine {
			if !self.double_click_edit {
				return;
			}
			actions::start_edit(click.message)
		} else {
			if !self.double_click_reply {
				return;
			}
			let without_ping = click.modifiers.contains(Modifiers::SHIFT) && !self.require_modifier;
			actions::start_reply(click.message, !without_ping)
		};
		if let Err(error) = outcome {
			log::debug!("double click action failed: {error:?}");
		}
	}

	fn delete(&self, click: &MessageClick) {
		let mine = client::get_message(click.message.channel_id, click.message.id)
			.ok()
			.flatten()
			.is_some_and(|message| message.mine);
		if mine && let Err(error) = actions::delete_message(click.message) {
			log::debug!("delete on click failed: {error:?}");
		}
	}
}

impl Plugin for MessageClickActions {
	fn init(&mut self) -> Result<(), String> {
		for key in SETTING_KEYS {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		self.watch();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& SETTING_KEYS.contains(&key.as_str())
		{
			self.set(&key, &value);
			self.watch();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"enableDeleteOnClick":{"type":"boolean","title":"Delete on Ctrl Alt click","description":"Delete your message when you Ctrl and Alt click it. Vencord uses Backspace and click, which plugins cannot detect","default":false},"enableDoubleClickToEdit":{"type":"boolean","title":"Double click to edit","description":"Enable double click to edit","default":true},"enableDoubleClickToReply":{"type":"boolean","title":"Double click to reply","description":"Enable double click to reply","default":true},"requireModifier":{"type":"boolean","title":"Require Ctrl or Shift","description":"Only do double click actions when Shift or Ctrl is held","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for MessageClickActions {
	fn on_message_click(&mut self, click: MessageClick) {
		match click.kind {
			ClickKind::Double => self.double_click(&click),
			ClickKind::Single => self.delete(&click),
			ClickKind::Middle => {}
		}
	}
}

export_plugin!(MessageClickActions, ext);
