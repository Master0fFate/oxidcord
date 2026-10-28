use std::collections::HashMap;

use plugin_sdk::model::{Badge, ClientEvent, Color, DisplayOverrideFull, Presence};
use plugin_sdk::{Plugin, PluginExt, User, data, export_plugin};

#[derive(Default)]
struct ForceOwnerCrown {
	owners: HashMap<u64, u64>,
}

fn crown() -> Badge {
	Badge {
		icon: Some("crown".to_owned()),
		image: None,
		label: None,
		tooltip: "Server owner".to_owned(),
		color: Some(Color::Token("warning".to_owned())),
		key: None,
	}
}

impl ForceOwnerCrown {
	fn owner_of(&mut self, guild_id: u64) -> Option<u64> {
		if let Some(owner) = self.owners.get(&guild_id) {
			return Some(*owner);
		}
		let owner = data::guild_detail(guild_id)
			.ok()
			.flatten()
			.map(|detail| detail.owner_id)
			.filter(|owner| *owner != 0)?;
		self.owners.insert(guild_id, owner);
		Some(owner)
	}
}

impl Plugin for ForceOwnerCrown {}

impl PluginExt for ForceOwnerCrown {
	fn display_user_full(
		&mut self,
		guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		(self.owner_of(guild_id?)? == user.id).then(|| DisplayOverrideFull {
			name: None,
			suffix: None,
			color: None,
			badges: vec![crown()],
			avatar: None,
			banner: None,
			decoration: None,
			pronouns: None,
		})
	}

	fn on_client_event(&mut self, event: ClientEvent) {
		match event {
			ClientEvent::Ready => self.owners.clear(),
			ClientEvent::GuildLeft(guild_id) | ClientEvent::GuildRemoved(guild_id) => {
				self.owners.remove(&guild_id);
			}
			_ => {}
		}
	}
}

export_plugin!(ForceOwnerCrown, ext);
