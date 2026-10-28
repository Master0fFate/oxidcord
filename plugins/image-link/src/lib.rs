use plugin_sdk::model::MessageFull;
use plugin_sdk::patch::Patch;
use plugin_sdk::view;
use plugin_sdk::{Md, Plugin, PluginExt, export_plugin};

const LABEL_CHARS: usize = 100;

#[derive(Default)]
struct ImageLink;

impl Plugin for ImageLink {}

impl PluginExt for ImageLink {
	fn intercept_message(&mut self, msg: &MessageFull, _content: Vec<Md>) -> Patch {
		let content = msg.view.content.trim();
		let lone_link = (content.starts_with("https://") || content.starts_with("http://"))
			&& !content.contains(char::is_whitespace);
		let previewed = msg.embeds.iter().any(|embed| {
			embed.url.as_deref() == Some(content) && matches!(embed.kind.as_str(), "image" | "gifv")
		});
		if !lone_link || !previewed {
			return Patch::keep();
		}
		let label: String = content.chars().take(LABEL_CHARS).collect();
		Patch::keep().accessory([view::link(label, content)])
	}
}

export_plugin!(ImageLink, ext);
