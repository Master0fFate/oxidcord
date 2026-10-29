use std::collections::HashMap;

use plugin_sdk::model::{Badge, Color, DisplayOverrideFull, Presence, RoleInfo};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view;
use plugin_sdk::{Event, Plugin, PluginExt, User, data, export_plugin, host, log, mount, now_ms, storage};

const BOTH_STYLES_KEY: &str = "bothStyles";
const COPY_KEY: &str = "copy";
const ROLES_TTL_MS: i64 = 30_000;
const MAX_TRACKED: usize = 2048;
const TOAST_MS: u32 = 2000;

#[derive(Default)]
struct BetterRoleDot {
	both_styles: bool,
	roles: HashMap<u64, (i64, Vec<RoleInfo>)>,
	colors: HashMap<u64, u32>,
}

fn rgba(color: u32) -> Color {
	Color::Rgba(color << 8 | 0xff)
}

impl BetterRoleDot {
	fn top_colored_role(&mut self, guild_id: u64, member_roles: &[u64]) -> Option<(String, u32)> {
		let now = now_ms();
		let entry = self.roles.entry(guild_id).or_default();
		if now - entry.0 > ROLES_TTL_MS {
			*entry = (now, data::list_roles(guild_id).unwrap_or_default());
		}
		entry
			.1
			.iter()
			.filter(|role| role.color != 0 && member_roles.contains(&role.id))
			.max_by_key(|role| role.position)
			.map(|role| (role.name.clone(), role.color))
	}

	fn remember(&mut self, user_id: u64, color: u32) {
		if self.colors.len() >= MAX_TRACKED {
			self.colors.clear();
		}
		self.colors.insert(user_id, color);
	}
}

impl Plugin for BetterRoleDot {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{BOTH_STYLES_KEY}")) {
			self.both_styles = matches!(String::from_utf8_lossy(&saved).trim(), "true" | "1" | "on");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == BOTH_STYLES_KEY
		{
			self.both_styles = matches!(value.trim(), "true" | "1" | "on");
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"bothStyles":{"type":"boolean","title":"Both styles","description":"Show both the role dot and the colored name","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for BetterRoleDot {
	fn display_user_full(
		&mut self,
		guild_id: Option<u64>,
		user: &User,
		_presence: Option<&Presence>,
	) -> Option<DisplayOverrideFull> {
		let guild_id = guild_id?;
		let member = data::member_info(guild_id, user.id).ok().flatten()?;
		let (name, color) = self.top_colored_role(guild_id, &member.roles)?;
		self.remember(user.id, color);
		Some(DisplayOverrideFull {
			name: None,
			suffix: None,
			color: self.both_styles.then(|| rgba(color)),
			badges: vec![Badge {
				icon: Some("circle".to_owned()),
				image: None,
				label: None,
				tooltip: name,
				color: Some(rgba(color)),
				key: Some(COPY_KEY.to_owned()),
			}],
			avatar: None,
			banner: None,
			decoration: None,
			pronouns: None,
		})
	}

	fn on_badge_click(&mut self, user_id: u64, key: &str) {
		let Some(color) = self.colors.get(&user_id).filter(|_| key == COPY_KEY) else {
			return;
		};
		let hex = format!("#{color:06x}");
		match host::clipboard_write(&hex) {
			Ok(()) => {
				if let Err(error) = mount::toast([view::text(format!("Copied {hex}"))], TOAST_MS) {
					log::debug!("could not show the toast: {error:?}");
				}
			}
			Err(error) => log::warn!("could not copy the color: {error:?}"),
		}
	}
}

export_plugin!(BetterRoleDot, ext);
