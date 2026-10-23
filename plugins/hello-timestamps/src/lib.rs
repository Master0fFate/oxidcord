use plugin_sdk::ui::{self, button};
use plugin_sdk::{Md, MessageView, Plugin, Ui, export_plugin, host, log};

const COPY_ID_ACTION: &str = "copy-id";
const MESSAGE_ACTION_SLOT: &str = "message-action";

#[derive(Default)]
struct HelloTimestamps;

impl Plugin for HelloTimestamps {
	fn init(&mut self) -> Result<(), String> {
		let slot = [Ui::from(button(COPY_ID_ACTION, "Copy ID").icon("copy"))];
		if let Err(error) = ui::register_slot(MESSAGE_ACTION_SLOT, &slot) {
			log::warn!("could not register the {MESSAGE_ACTION_SLOT} slot: {error:?}");
		}
		Ok(())
	}

	fn render_message(&mut self, msg: &MessageView, mut ast: Vec<Md>) -> Option<Vec<Md>> {
		let elapsed_secs = (host::now_ms() - msg.timestamp_ms).max(0) / 1000;
		ast.push(Md::subtext(format!("sent {}", relative_time(elapsed_secs))));
		Some(ast)
	}

	fn on_ui_action(&mut self, id: &str, payload: &str) -> Vec<Ui> {
		if id == COPY_ID_ACTION
			&& let Err(error) = host::clipboard_write(payload)
		{
			log::warn!("copy failed: {error:?}");
		}
		Vec::new()
	}
}

fn relative_time(elapsed_secs: i64) -> String {
	const UNITS: [(i64, &str); 4] = [(86_400, "day"), (3_600, "hour"), (60, "minute"), (1, "second")];
	if elapsed_secs < 5 {
		return "just now".to_owned();
	}
	let (size, name) = UNITS
		.iter()
		.copied()
		.find(|(size, _)| elapsed_secs >= *size)
		.unwrap_or((1, "second"));
	let count = elapsed_secs / size;
	let plural = if count == 1 { "" } else { "s" };
	format!("{count} {name}{plural} ago")
}

export_plugin!(HelloTimestamps);
