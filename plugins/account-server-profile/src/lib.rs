use plugin_sdk::model::{MemberInfo, Space, TextSize, UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, Plugin, PluginExt, Reply, User, data, export_plugin, host, log, menu, mount, storage,
};

const PRIORITIZE_KEY: &str = "prioritizeServerProfile";
const MOUNT_KEY: &str = "server-profile";
const OPEN_BUTTON: &str = "open";
const MENU_ITEM: &str = "server-profile";
const AVATAR_PX: u32 = 64;
const TOAST_MS: u32 = 3000;

#[derive(Default)]
struct AccountServerProfile {
	prioritize: bool,
	guild_id: Option<u64>,
}

fn notice(text: &str) {
	if let Err(error) = mount::toast([view::text(text)], TOAST_MS) {
		log::debug!("could not show the toast: {error:?}");
	}
}

fn role_names(guild_id: u64, member: &MemberInfo) -> Vec<String> {
	let mut roles = data::list_roles(guild_id).unwrap_or_default();
	roles.retain(|role| member.roles.contains(&role.id));
	roles.sort_by_key(|role| std::cmp::Reverse(role.position));
	roles.into_iter().map(|role| role.name).collect()
}

fn profile_window(me: &User, member: &MemberInfo, roles: Vec<String>) -> Node {
	let name = member.nick.as_deref().unwrap_or(&me.display_name);
	let mut role_row = view::row().gap(Space::Xs).wrap();
	if roles.is_empty() {
		role_row = role_row.child(view::text("None").size(TextSize::Xs).muted());
	}
	for role in roles {
		role_row = role_row.child(view::badge(role));
	}
	let mut body = view::column()
		.gap(Space::Md)
		.child(
			view::row()
				.gap(Space::Md)
				.child(view::avatar(me.id, AVATAR_PX))
				.child(
					view::column()
						.child(view::heading(name))
						.child(view::text(format!("@{}", me.username)).size(TextSize::Xs).muted()),
				),
		)
		.child(view::divider())
		.child(view::text("Roles").size(TextSize::Xs).muted())
		.child(role_row);
	if let Some(joined_ms) = member.joined_ms {
		body = body.child(
			view::row()
				.gap(Space::Sm)
				.child(view::text("Joined").size(TextSize::Xs).muted())
				.child(view::timestamp(joined_ms, "D")),
		);
	}
	body
}

impl AccountServerProfile {
	fn label(&self) -> Option<String> {
		let me = host::current_user().ok()?;
		let nick = self
			.guild_id
			.filter(|_| self.prioritize)
			.and_then(|guild_id| data::member_info(guild_id, me.id).ok().flatten())
			.and_then(|member| member.nick);
		Some(nick.unwrap_or(me.display_name))
	}

	fn tree(&self) -> Vec<Node> {
		let mut row = view::row().gap(Space::Sm);
		if let Some(label) = self.label() {
			row = row.child(view::text(label).size(TextSize::Xs).bold());
		}
		vec![row.child(view::icon_button(OPEN_BUTTON, "user-round", "Server profile"))]
	}

	fn refresh_mount(&self) {
		if let Err(error) = mount::update(mount::ACCOUNT_PANEL, MOUNT_KEY, self.tree()) {
			log::debug!("could not update the account row: {error:?}");
		}
	}

	fn open_profile(&self) {
		let Ok(Some((Some(guild_id), _))) = host::current_channel() else {
			return notice("No server open");
		};
		let Ok(me) = host::current_user() else {
			return notice("Not available right now");
		};
		let Ok(Some(member)) = data::member_info(guild_id, me.id) else {
			return notice("Your server profile is not loaded");
		};
		let roles = role_names(guild_id, &member);
		if let Err(error) = mount::open_modal("Server profile", [profile_window(&me, &member, roles)]) {
			log::warn!("could not open the profile window: {error:?}");
		}
	}
}

impl Plugin for AccountServerProfile {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{PRIORITIZE_KEY}")) {
			self.prioritize = matches!(String::from_utf8_lossy(&saved).trim(), "true" | "1" | "on");
		}
		self.guild_id = host::current_channel()
			.ok()
			.flatten()
			.and_then(|(guild_id, _)| guild_id);
		let added = mount::mount(
			mount::ACCOUNT_PANEL,
			MOUNT_KEY,
			self.tree(),
			mount::Options::new("Server profile"),
		)
		.and_then(|()| {
			menu::set(
				menu::ACCOUNT_PANEL,
				[menu::item(MENU_ITEM, "Server profile").icon("user")],
			)
		});
		if let Err(error) = added {
			log::warn!("could not add the server profile controls: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::ChannelSwitched(switch) => {
				self.guild_id = switch.guild_id;
				self.refresh_mount();
			}
			Event::SettingChanged(SettingChange { key, value }) if key == PRIORITIZE_KEY => {
				self.prioritize = matches!(value.trim(), "true" | "1" | "on");
				self.refresh_mount();
			}
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"prioritizeServerProfile":{"type":"boolean","title":"Prioritize server profile","description":"Show your server nickname instead of your account name in the row above the account panel","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for AccountServerProfile {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind == ViewEventKind::Click
			&& event.point == mount::ACCOUNT_PANEL
			&& event.node == OPEN_BUTTON
		{
			self.open_profile();
		}
		Reply::none()
	}

	fn on_menu_item(&mut self, menu: &str, key: &str, _context: &UiContext) {
		if menu == menu::ACCOUNT_PANEL && key == MENU_ITEM {
			self.open_profile();
		}
	}
}

export_plugin!(AccountServerProfile, ext);
