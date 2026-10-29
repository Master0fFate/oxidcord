use plugin_sdk::model::{GuildDetail, Space, TextSize, UiContext};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{HostError, Plugin, PluginExt, data, export_plugin, host, log, menu, mount};

const INFO_ITEM: &str = "info";
const CDN: &str = "https://cdn.discordapp.com";
const ICON_PX: u32 = 64;
const BANNER_WIDTH_PX: u32 = 480;
const BANNER_HEIGHT_PX: u32 = 192;
const MAX_FEATURES: usize = 24;
const TOAST_MS: u32 = 3000;

#[derive(Default)]
struct ServerInfo;

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

fn field(label: &str, value: Node) -> Node {
	view::column()
		.gap(Space::Xs)
		.child(view::text(label).size(TextSize::Xs).muted())
		.child(value)
}

fn readable(feature: &str) -> String {
	let lower = feature.replace('_', " ").to_lowercase();
	let mut chars = lower.chars();
	chars
		.next()
		.map(|first| first.to_uppercase().chain(chars).collect())
		.unwrap_or_default()
}

fn header(detail: &GuildDetail) -> Node {
	let mut titles = view::column()
		.gap(Space::Xs)
		.child(view::heading(detail.name.clone()));
	if let Some(description) = &detail.description {
		titles = titles.child(view::text(description.clone()).muted());
	}
	let mut row = view::row().gap(Space::Md);
	if let Some(hash) = &detail.icon {
		let url = format!("{CDN}/icons/{}/{hash}.webp?size=256", detail.id);
		row = row.child(view::image(url).image_size(ICON_PX, ICON_PX));
	}
	row.child(titles)
}

fn body(detail: &GuildDetail, channels: usize) -> Node {
	let mut column = view::column().gap(Space::Md);
	if let Some(hash) = &detail.banner {
		let url = format!("{CDN}/banners/{}/{hash}.webp?size=1024", detail.id);
		column = column.child(view::image(url).image_size(BANNER_WIDTH_PX, BANNER_HEIGHT_PX));
	}
	column = column
		.child(header(detail))
		.child(view::divider())
		.child(field("Owner", view::user_chip(detail.owner_id)))
		.child(field("Created", view::timestamp(detail.created_ms, "F")));
	if let Some(members) = detail.member_count {
		column = column.child(field("Members", view::text(members.to_string())));
	}
	if let Some(online) = detail.presence_count {
		column = column.child(field("Online", view::text(online.to_string())));
	}
	column = column
		.child(field(
			"Boosts",
			view::text(format!("{} (level {})", detail.boost_count, detail.boost_tier)),
		))
		.child(field("Channels", view::text(channels.to_string())));
	if let Some(code) = &detail.vanity {
		column = column.child(field(
			"Vanity link",
			view::link(format!("discord.gg/{code}"), format!("https://discord.gg/{code}")),
		));
	}
	if !detail.features.is_empty() {
		let badges = detail
			.features
			.iter()
			.take(MAX_FEATURES)
			.fold(view::row().gap(Space::Xs).wrap(), |row, feature| {
				row.child(view::badge(readable(feature)))
			});
		column = column.child(field("Features", badges));
	}
	column
}

impl ServerInfo {
	fn show(&self, guild_id: u64) -> Result<(), String> {
		let detail = data::guild_detail(guild_id)
			.map_err(|error| describe(&error))?
			.ok_or("That server is not available")?;
		let channels = host::list_channels(Some(guild_id))
			.map(|list| list.len())
			.unwrap_or(0);
		mount::open_modal(&detail.name, [view::scroll().child(body(&detail, channels))])
			.map_err(|error| describe(&error))
	}
}

impl Plugin for ServerInfo {
	fn init(&mut self) -> Result<(), String> {
		let item = menu::item(INFO_ITEM, "Server info").icon("info");
		if let Err(error) = menu::set(menu::SERVER, [item]) {
			log::warn!("could not add the menu entry: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for ServerInfo {
	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		if menu != menu::SERVER || key != INFO_ITEM {
			return;
		}
		if let Some(guild_id) = context.guild_id
			&& let Err(text) = self.show(guild_id)
		{
			notice(&text);
		}
	}
}

export_plugin!(ServerInfo, ext);
