use plugin_sdk::model::ClientOption;
use plugin_sdk::{Plugin, actions, export_plugin, log};

#[derive(Default)]
struct NoTypingAnimation;

impl Plugin for NoTypingAnimation {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = actions::set_client_option(ClientOption::TypingAnimation(false)) {
			log::warn!("could not turn the typing animation off: {error:?}");
		}
		Ok(())
	}
}

export_plugin!(NoTypingAnimation);
