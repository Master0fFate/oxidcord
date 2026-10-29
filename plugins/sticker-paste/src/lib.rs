use plugin_sdk::model::{Align, ButtonVariant, Length, Space, StickerInfo, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Plugin, PluginExt, Reply, actions, client, export_plugin, log, mount};

const TOOLBAR_KEY: &str = "stickers";
const OPEN_BUTTON: &str = "open";
const PICK_PREFIX: &str = "pick:";
const MAX_STICKERS: usize = 60;
const LOTTIE_FORMAT: u8 = 3;
const THUMBNAIL_PX: u32 = 64;
const CELL_PX: u32 = 84;
const GRID_HEIGHT: u32 = 360;
const MAX_LABEL_CHARS: usize = 12;

#[derive(Default)]
struct StickerPaste;

fn label(name: &str) -> String {
	name.chars().take(MAX_LABEL_CHARS).collect()
}

fn cell(sticker: &StickerInfo) -> Node {
	view::column()
		.width(Length::Px(CELL_PX))
		.align(Align::Center)
		.gap(Space::Xs)
		.child(view::image(client::sticker_url(sticker, THUMBNAIL_PX)).image_size(THUMBNAIL_PX, THUMBNAIL_PX))
		.child(
			view::button(format!("{PICK_PREFIX}{}", sticker.id), label(&sticker.name))
				.variant(ButtonVariant::Ghost)
				.tooltip(sticker.name.as_str()),
		)
}

fn usable_stickers(guild_id: Option<u64>) -> Vec<StickerInfo> {
	let mut stickers = Vec::new();
	let sources = guild_id.into_iter().map(Some).chain([None]);
	for source in sources {
		for sticker in client::list_stickers(source).unwrap_or_default() {
			if sticker.format != LOTTIE_FORMAT
				&& !stickers.iter().any(|known: &StickerInfo| known.id == sticker.id)
			{
				stickers.push(sticker);
			}
		}
	}
	stickers.truncate(MAX_STICKERS);
	stickers
}

impl Plugin for StickerPaste {
	fn init(&mut self) -> Result<(), String> {
		let button = view::icon_button(OPEN_BUTTON, "sticker", "Paste a sticker");
		let options = mount::Options::new("Sticker paste");
		if let Err(error) = mount::mount(mount::COMPOSER_TOOLBAR, TOOLBAR_KEY, [button], options) {
			log::warn!("could not add the composer button: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for StickerPaste {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if !matches!(event.kind, ViewEventKind::Click) {
			return Reply::none();
		}
		if event.node == OPEN_BUTTON {
			let stickers = usable_stickers(event.context.guild_id);
			if stickers.is_empty() {
				return Reply::none().toast("No stickers available");
			}
			let grid = view::row()
				.wrap()
				.gap(Space::Sm)
				.children(stickers.iter().map(cell));
			let body = view::scroll()
				.height(Length::Px(GRID_HEIGHT))
				.padding(Space::Md)
				.child(grid);
			if let Err(error) = mount::open_modal("Stickers", [body]) {
				log::warn!("could not open the sticker picker: {error:?}");
			}
			return Reply::none();
		}
		let Some(id) = event
			.node
			.strip_prefix(PICK_PREFIX)
			.and_then(|id| id.parse().ok())
		else {
			return Reply::none();
		};
		match actions::composer_attach_sticker(id) {
			Ok(()) => Reply::none().close(),
			Err(error) => {
				log::warn!("could not attach the sticker: {error:?}");
				Reply::none().toast("Could not attach the sticker")
			}
		}
	}
}

export_plugin!(StickerPaste, ext);
