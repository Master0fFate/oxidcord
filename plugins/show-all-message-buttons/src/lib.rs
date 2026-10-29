use plugin_sdk::model::ClientOption;
use plugin_sdk::{Plugin, actions, export_plugin, log};

#[derive(Default)]
struct ShowAllMessageButtons;

impl Plugin for ShowAllMessageButtons {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = actions::set_client_option(ClientOption::MessageToolbarExpanded(true)) {
			log::warn!("could not expand the message toolbar: {error:?}");
		}
		Ok(())
	}
}

export_plugin!(ShowAllMessageButtons);
