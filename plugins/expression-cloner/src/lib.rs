use plugin_sdk::model::{
	ButtonVariant, EmojiRef, Space, StickerInfo, TextSize, UiContext, ViewEvent, ViewEventKind,
};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	GuildSummary, HostError, Plugin, PluginExt, Reply, actions, client, data, export_plugin, host, log, menu,
	mount, ui,
};

const EMOJI_ITEM: &str = "clone-emoji";
const STICKER_ITEM: &str = "clone-sticker";
const MODAL_POINT: &str = "modal";
const FORM: &str = "clone";
const NAME_FIELD: &str = "name";
const GUILD_FIELD: &str = "guild";
const SUBMIT: &str = "submit";
const ADMINISTRATOR: u64 = 1 << 3;
const CREATE_GUILD_EXPRESSIONS: u64 = 1 << 43;
const MAX_GUILDS: usize = 100;
const PNG_FORMAT: u8 = 1;
const PREVIEW_PX: u32 = 64;
const TOAST_MS: u32 = 3000;

#[derive(Clone)]
enum Subject {
	Emoji(EmojiRef),
	Sticker(u64),
}

#[derive(Default)]
struct ExpressionCloner {
	subject: Option<Subject>,
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

fn default_name(emoji: &EmojiRef) -> &str {
	match emoji.name.rsplit_once('~') {
		Some((base, tail)) if !tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit()) => base,
		_ => &emoji.name,
	}
}

fn valid_name(name: &str) -> bool {
	(2..=32).contains(&name.len())
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn can_create(guild: &GuildSummary) -> bool {
	guild.owner
		|| host::list_channels(Some(guild.id))
			.ok()
			.and_then(|channels| channels.first().map(|channel| channel.id))
			.and_then(|channel| data::channel_permissions(channel, None).ok())
			.is_some_and(|bits| bits & (ADMINISTRATOR | CREATE_GUILD_EXPRESSIONS) != 0)
}

fn candidates() -> Vec<GuildSummary> {
	let mut guilds: Vec<GuildSummary> = host::list_guilds()
		.unwrap_or_default()
		.into_iter()
		.filter(can_create)
		.take(MAX_GUILDS)
		.collect();
	guilds.sort_by_key(|guild| guild.name.to_lowercase());
	guilds
}

fn preview(subject: &Subject) -> Node {
	match subject {
		Subject::Emoji(emoji) => view::emoji(emoji.clone(), PREVIEW_PX),
		Subject::Sticker(id) => {
			let known = client::list_stickers(None)
				.unwrap_or_default()
				.into_iter()
				.find(|sticker| sticker.id == *id);
			let sticker = known.unwrap_or(StickerInfo {
				id: *id,
				name: String::new(),
				format: PNG_FORMAT,
				guild_id: None,
			});
			view::image(client::sticker_url(&sticker, PREVIEW_PX * 2)).image_size(PREVIEW_PX, PREVIEW_PX)
		}
	}
}

fn dialog(subject: &Subject, guilds: &[GuildSummary]) -> Vec<Node> {
	if guilds.is_empty() {
		return vec![view::text("No server where you can add expressions").muted()];
	}
	let mut select = view::select(GUILD_FIELD, "Server");
	for guild in guilds {
		select = select.option(guild.id.to_string(), guild.name.clone());
	}
	let mut form = view::form()
		.key(FORM)
		.child(select.choose(guilds[0].id.to_string()));
	if let Subject::Emoji(emoji) = subject {
		form = form.child(view::input(NAME_FIELD, "Name").value(default_name(emoji)));
	}
	let form = form.child(
		view::button(SUBMIT, "Clone")
			.icon("copy-plus")
			.variant(ButtonVariant::Solid)
			.submit(),
	);
	vec![
		view::column()
			.gap(Space::Md)
			.child(preview(subject))
			.child(
				view::text("Pick the server to add it to")
					.size(TextSize::Xs)
					.muted(),
			)
			.child(form.gap(Space::Md)),
	]
}

fn clone_into(subject: &Subject, guild_id: u64, name: &str) -> Result<(), HostError> {
	match subject {
		Subject::Emoji(emoji) => actions::clone_emoji(guild_id, emoji, name),
		Subject::Sticker(id) => actions::clone_sticker(guild_id, *id),
	}
}

impl ExpressionCloner {
	fn open(&mut self, subject: Subject) {
		let guilds = candidates();
		if let Err(error) = mount::open_modal("Clone to server", dialog(&subject, &guilds)) {
			return log::warn!("could not open the clone window: {error:?}");
		}
		self.subject = Some(subject);
	}

	fn submit(&mut self, values: &[(String, String)]) -> Reply {
		let field = |key: &str| {
			values
				.iter()
				.find(|(name, _)| name == key)
				.map(|(_, value)| value.trim())
		};
		let Some(subject) = self.subject.clone() else {
			return Reply::none().close();
		};
		let Some(guild_id) = field(GUILD_FIELD).and_then(|value| value.parse().ok()) else {
			return Reply::none().toast("Pick a server");
		};
		let name = field(NAME_FIELD).unwrap_or_default();
		if matches!(subject, Subject::Emoji(_)) && !valid_name(name) {
			return Reply::none().toast("The name needs 2 to 32 letters, digits or underscores");
		}
		match clone_into(&subject, guild_id, name) {
			Ok(()) => Reply::none().close().toast("Cloning"),
			Err(error) => Reply::none().toast(describe(&error)),
		}
	}
}

impl Plugin for ExpressionCloner {
	fn init(&mut self) -> Result<(), String> {
		let added = menu::set(
			menu::EMOJI,
			[menu::item(EMOJI_ITEM, "Clone to server").icon("copy-plus")],
		)
		.and_then(|()| {
			menu::set(
				menu::STICKER,
				[menu::item(STICKER_ITEM, "Clone to server").icon("copy-plus")],
			)
		});
		if let Err(error) = added {
			log::warn!("could not add the menu entries: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for ExpressionCloner {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		match (&event.kind, event.point.as_str(), event.node.as_str()) {
			(ViewEventKind::Submit(values), MODAL_POINT, FORM) => self.submit(values),
			_ => Reply::none(),
		}
	}

	fn on_menu_item(&mut self, menu: &str, key: &str, _context: &UiContext) {
		let Some(detail) = ui::context_detail() else {
			return;
		};
		match (menu, key) {
			(menu::EMOJI, EMOJI_ITEM) => match detail.emoji {
				Some(emoji) if emoji.id.is_some() => self.open(Subject::Emoji(emoji)),
				_ => notice("Only custom emoji can be cloned"),
			},
			(menu::STICKER, STICKER_ITEM) => {
				if let Some(id) = detail.sticker_id {
					self.open(Subject::Sticker(id));
				}
			}
			_ => {}
		}
	}
}

export_plugin!(ExpressionCloner, ext);
