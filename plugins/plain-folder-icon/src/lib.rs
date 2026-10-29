use plugin_sdk::model::ClientOption;
use plugin_sdk::{Plugin, actions, export_plugin, log};

#[derive(Default)]
struct PlainFolderIcon;

impl Plugin for PlainFolderIcon {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = actions::set_client_option(ClientOption::PlainFolderIcons(true)) {
			log::warn!("could not switch to plain folder icons: {error:?}");
		}
		Ok(())
	}
}

export_plugin!(PlainFolderIcon);
