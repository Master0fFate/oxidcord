use plugin_sdk::model::{ChannelClick, ClickKind, ClickWatch, Modifiers};
use plugin_sdk::{Plugin, PluginExt, actions, data, export_plugin, host, log, ui};

const VOICE_KIND: u32 = 2;
const STAGE_KIND: u32 = 13;

#[derive(Default)]
struct VcDoubleClick;

fn is_voice(channel_id: u64) -> bool {
	data::channel_detail(channel_id)
		.ok()
		.flatten()
		.is_some_and(|channel| matches!(channel.kind, VOICE_KIND | STAGE_KIND))
}

fn connected_to(channel_id: u64) -> bool {
	data::voice_detail()
		.ok()
		.flatten()
		.is_some_and(|detail| detail.base.channel_id == channel_id)
}

fn open(click: &ChannelClick) {
	if let Err(error) = host::open_channel(click.guild_id, click.channel_id) {
		log::debug!("could not open the channel: {error:?}");
	}
}

impl Plugin for VcDoubleClick {
	fn init(&mut self) -> Result<(), String> {
		let watch = ClickWatch {
			kinds: vec![ClickKind::Single, ClickKind::Double],
			modifiers: Modifiers::empty(),
			targets: Vec::new(),
		};
		if let Err(error) = ui::watch_channel_clicks(&watch) {
			log::warn!("could not watch channel clicks: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for VcDoubleClick {
	fn on_channel_click(&mut self, click: ChannelClick) {
		if !is_voice(click.channel_id) {
			open(&click);
			return;
		}
		if connected_to(click.channel_id) {
			open(&click);
		} else if click.kind == ClickKind::Double
			&& let Err(error) = actions::join_voice(click.guild_id, click.channel_id)
		{
			log::debug!("could not join the voice channel: {error:?}");
		}
	}
}

export_plugin!(VcDoubleClick, ext);
