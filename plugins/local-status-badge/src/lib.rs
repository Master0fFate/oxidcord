use plugin_sdk::ui::{self, badge, button, row, text};
use plugin_sdk::{Event, Plugin, Ui, export_plugin, host, log};

const STATUS_SLOT: &str = "status-bar";
const CYCLE_ACTION: &str = "cycle-badge-color";
const COLORS: [&str; 3] = ["#6666ff", "#23a559", "#f0b232"];

#[derive(Default)]
struct LocalStatusBadge {
	channel_id: Option<u64>,
	color: usize,
}

impl LocalStatusBadge {
	fn tree(&self) -> Vec<Ui> {
		let label = match self.channel_id {
			Some(id) => format!("channel {id}"),
			None => "no channel".to_owned(),
		};
		vec![
			row()
				.gap(8)
				.child(text(label))
				.child(badge("local").color(COLORS[self.color]))
				.child(button(CYCLE_ACTION, "Color").icon("palette"))
				.into(),
		]
	}

	fn publish(&self) {
		if let Err(error) = ui::register_slot(STATUS_SLOT, &self.tree()) {
			log::warn!("could not register the {STATUS_SLOT} slot: {error:?}");
		}
	}
}

impl Plugin for LocalStatusBadge {
	fn init(&mut self) -> Result<(), String> {
		self.channel_id = host::current_channel().ok().flatten().map(|(_, channel)| channel);
		self.publish();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::ChannelSwitched(switch) = event {
			self.channel_id = Some(switch.channel_id);
			self.publish();
		}
	}

	fn on_ui_action(&mut self, id: &str, _payload: &str) -> Vec<Ui> {
		if id != CYCLE_ACTION {
			return Vec::new();
		}
		self.color = (self.color + 1) % COLORS.len();
		self.tree()
	}
}

export_plugin!(LocalStatusBadge);
