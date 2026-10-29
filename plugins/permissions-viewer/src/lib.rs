use plugin_sdk::model::{Length, MemberInfo, RoleInfo, Space, TextSize, Tone, UiContext};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Color, Event, HostError, Plugin, PluginExt, data, export_plugin, host, log, menu, mount, storage,
};

const SORT_KEY: &str = "permissionsSortOrder";
const PERMISSIONS_ITEM: &str = "permissions";
const SERVER_ITEM: &str = "roles";
const LIST_HEIGHT_PX: u32 = 320;
const ICON_PX: u32 = 16;
const TOAST_MS: u32 = 3000;

const PERMISSIONS: [(u8, &str); 51] = [
	(0, "Create Invite"),
	(1, "Kick Members"),
	(2, "Ban Members"),
	(3, "Administrator"),
	(4, "Manage Channels"),
	(5, "Manage Server"),
	(6, "Add Reactions"),
	(7, "View Audit Log"),
	(8, "Priority Speaker"),
	(9, "Video"),
	(10, "View Channel"),
	(11, "Send Messages"),
	(12, "Send TTS Messages"),
	(13, "Manage Messages"),
	(14, "Embed Links"),
	(15, "Attach Files"),
	(16, "Read Message History"),
	(17, "Mention Everyone"),
	(18, "Use External Emoji"),
	(19, "View Server Insights"),
	(20, "Connect"),
	(21, "Speak"),
	(22, "Mute Members"),
	(23, "Deafen Members"),
	(24, "Move Members"),
	(25, "Use Voice Activity"),
	(26, "Change Nickname"),
	(27, "Manage Nicknames"),
	(28, "Manage Roles"),
	(29, "Manage Webhooks"),
	(30, "Manage Expressions"),
	(31, "Use Application Commands"),
	(32, "Request to Speak"),
	(33, "Manage Events"),
	(34, "Manage Threads"),
	(35, "Create Public Threads"),
	(36, "Create Private Threads"),
	(37, "Use External Stickers"),
	(38, "Send Messages in Threads"),
	(39, "Use Activities"),
	(40, "Timeout Members"),
	(41, "View Creator Analytics"),
	(42, "Use Soundboard"),
	(43, "Create Expressions"),
	(44, "Create Events"),
	(45, "Use External Sounds"),
	(46, "Send Voice Messages"),
	(49, "Create Polls"),
	(50, "Use External Apps"),
	(51, "Pin Messages"),
	(52, "Bypass Slow Mode"),
];

#[derive(Default)]
struct PermissionsViewer {
	lowest_first: bool,
}

fn describe(error: &HostError) -> String {
	match error {
		HostError::Denied(_) => "A permission is missing".to_owned(),
		HostError::RateLimited => "Too many requests, try again in a moment".to_owned(),
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

fn open(title: &str, body: Node) {
	if let Err(error) = mount::open_modal(title, [body]) {
		log::warn!("could not open the window: {error:?}");
	}
}

fn scrolling(rows: Node) -> Node {
	view::scroll().height(Length::Px(LIST_HEIGHT_PX)).child(rows)
}

fn permission_rows(bits: u64) -> Node {
	let mut rows = view::column().gap(Space::Xs);
	for (bit, name) in PERMISSIONS {
		let granted = bits & (1 << bit) != 0;
		let (icon, tone) = if granted {
			("check", Tone::Success)
		} else {
			("x", Tone::Danger)
		};
		rows = rows.child(
			view::row()
				.gap(Space::Sm)
				.child(view::icon(icon).tone(tone).icon_size(ICON_PX))
				.child(view::text(name)),
		);
	}
	rows
}

fn role_row(role: &RoleInfo) -> Node {
	let name = view::text(role.name.clone());
	let name = if role.color == 0 {
		name
	} else {
		let [_, red, green, blue] = role.color.to_be_bytes();
		name.color(Color::rgb(red, green, blue))
	};
	view::row().gap(Space::Sm).child(name).child(
		view::text(format!("position {}", role.position))
			.size(TextSize::Xs)
			.muted(),
	)
}

fn role_rows<'a>(roles: impl Iterator<Item = &'a RoleInfo>) -> Node {
	roles.fold(view::column().gap(Space::Xs), |rows, role| {
		rows.child(role_row(role))
	})
}

fn channel_line(channel_id: u64) -> Node {
	view::row()
		.gap(Space::Sm)
		.child(view::text("In").size(TextSize::Xs).muted())
		.child(view::channel_chip(channel_id))
}

fn channel_in(guild_id: u64) -> Option<u64> {
	if let Ok(Some((Some(guild), channel))) = host::current_channel()
		&& guild == guild_id
	{
		return Some(channel);
	}
	host::list_channels(Some(guild_id))
		.ok()?
		.first()
		.map(|channel| channel.id)
}

fn display_name(member: &MemberInfo) -> &str {
	member.nick.as_deref().unwrap_or(&member.user.display_name)
}

impl PermissionsViewer {
	fn sorted<'a>(&self, roles: impl Iterator<Item = &'a RoleInfo>) -> Vec<&'a RoleInfo> {
		let mut roles: Vec<&RoleInfo> = roles.collect();
		roles.sort_by_key(|role| role.position);
		if !self.lowest_first {
			roles.reverse();
		}
		roles
	}

	fn show_user(&self, guild_id: u64, user_id: u64) -> Result<(), String> {
		let member = data::member_info(guild_id, user_id)
			.map_err(|error| describe(&error))?
			.ok_or("That member is not loaded")?;
		let channel_id = channel_in(guild_id).ok_or("No channel to read permissions from")?;
		let bits = data::channel_permissions(channel_id, Some(user_id)).map_err(|error| describe(&error))?;
		let roles = data::list_roles(guild_id).map_err(|error| describe(&error))?;
		let held = self.sorted(
			roles
				.iter()
				.filter(|role| role.id != guild_id && member.roles.contains(&role.id)),
		);
		let body = view::tabs()
			.tab(
				"Permissions",
				view::column()
					.gap(Space::Sm)
					.child(channel_line(channel_id))
					.child(scrolling(permission_rows(bits))),
			)
			.tab("Roles", scrolling(role_rows(held.into_iter())));
		open(&format!("{} permissions", display_name(&member)), body);
		Ok(())
	}

	fn show_channel(&self, channel_id: u64) -> Result<(), String> {
		let bits = data::channel_permissions(channel_id, None).map_err(|error| describe(&error))?;
		let body = view::column()
			.gap(Space::Sm)
			.child(channel_line(channel_id))
			.child(scrolling(permission_rows(bits)));
		open("Your permissions", body);
		Ok(())
	}

	fn show_roles(&self, guild_id: u64) -> Result<(), String> {
		let roles = data::list_roles(guild_id).map_err(|error| describe(&error))?;
		open(
			"Roles",
			scrolling(role_rows(self.sorted(roles.iter()).into_iter())),
		);
		Ok(())
	}
}

impl Plugin for PermissionsViewer {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{SORT_KEY}")) {
			self.lowest_first = String::from_utf8_lossy(&saved).trim() == "lowest";
		}
		let menus = menu::set(
			menu::USER,
			[menu::item(PERMISSIONS_ITEM, "View permissions")
				.icon("shield")
				.when(menu::When::IN_GUILD)],
		)
		.and_then(|()| {
			menu::set(
				menu::CHANNEL,
				[menu::item(PERMISSIONS_ITEM, "View permissions")
					.icon("shield")
					.when(menu::When::IN_GUILD)],
			)
		})
		.and_then(|()| {
			menu::set(
				menu::SERVER,
				[menu::item(SERVER_ITEM, "View roles").icon("shield")],
			)
		});
		if let Err(error) = menus {
			log::warn!("could not add the menu entries: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == SORT_KEY
		{
			self.lowest_first = value.trim() == "lowest";
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"permissionsSortOrder":{"type":"string","title":"Role sort order","description":"The order roles are listed in","enum":["highest","lowest"],"default":"highest"}}}"#
			.to_owned()
	}
}

impl PluginExt for PermissionsViewer {
	fn on_menu_item(&mut self, menu: &str, _key: &str, context: &UiContext) {
		let shown = match (menu, context.guild_id, context.user_id, context.channel_id) {
			(menu::USER, Some(guild), Some(user), _) => self.show_user(guild, user),
			(menu::CHANNEL, _, _, Some(channel)) => self.show_channel(channel),
			(menu::SERVER, Some(guild), _, _) => self.show_roles(guild),
			_ => return,
		};
		if let Err(text) = shown {
			notice(&text);
		}
	}
}

export_plugin!(PermissionsViewer, ext);
