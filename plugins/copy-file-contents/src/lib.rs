use plugin_sdk::model::{AttachmentInfo, UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	HostError, MessageRef, Plugin, PluginExt, Reply, client, data, export_plugin, host, log, mount, ui,
};

const BUTTON: &str = "copy";
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;
const TEXT_EXTENSIONS: [&str; 42] = [
	"txt", "md", "markdown", "json", "jsonc", "toml", "yaml", "yml", "xml", "csv", "tsv", "log", "ini",
	"cfg", "conf", "env", "js", "mjs", "ts", "tsx", "jsx", "py", "rs", "go", "java", "kt", "c", "h", "cpp",
	"hpp", "cs", "php", "rb", "lua", "sql", "html", "css", "scss", "sh", "bat", "ps1", "swift",
];

#[derive(Default)]
struct CopyFileContents;

fn describe(error: &HostError) -> String {
	match error {
		HostError::Denied(_) => "A permission is missing".to_owned(),
		HostError::RateLimited => "Too many requests, try again in a moment".to_owned(),
		HostError::Invalid(reason) => reason.clone(),
		HostError::Timeout => "Discord took too long".to_owned(),
		HostError::Unavailable => "Not available right now".to_owned(),
	}
}

fn message_ref(context: &UiContext) -> Option<MessageRef> {
	Some(MessageRef {
		id: context.message_id?,
		channel_id: context.channel_id?,
		guild_id: context.guild_id,
	})
}

fn is_text(attachment: &AttachmentInfo) -> bool {
	if attachment.size >= MAX_FILE_BYTES {
		return false;
	}
	if attachment
		.content_type
		.as_deref()
		.is_some_and(|kind| kind.starts_with("text/"))
	{
		return true;
	}
	attachment
		.filename
		.rsplit_once('.')
		.is_some_and(|(_, extension)| {
			TEXT_EXTENSIONS
				.iter()
				.any(|known| extension.eq_ignore_ascii_case(known))
		})
}

fn is_copyable(message: MessageRef, attachment_id: u64) -> bool {
	client::get_message(message.channel_id, message.id)
		.ok()
		.flatten()
		.is_some_and(|full| {
			full.attachments
				.iter()
				.any(|attachment| attachment.id == attachment_id && is_text(attachment))
		})
}

fn copy(message: MessageRef, attachment_id: u64) -> Result<&'static str, HostError> {
	let bytes = data::attachment_bytes(message, attachment_id)?;
	let text = String::from_utf8_lossy(&bytes);
	let end = text.floor_char_boundary(MAX_CLIPBOARD_BYTES);
	host::clipboard_write(&text[..end])?;
	Ok(if end < text.len() {
		"Copied the first 64 KB"
	} else {
		"Copied"
	})
}

impl Plugin for CopyFileContents {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = mount::provide(mount::ATTACHMENT_ACTION, mount::Options::new("Copy")) {
			log::warn!("could not add the copy button: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for CopyFileContents {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		if point != mount::ATTACHMENT_ACTION {
			return None;
		}
		let attachment_id = ui::context_detail()?.attachment_id?;
		is_copyable(message_ref(context)?, attachment_id)
			.then(|| vec![view::icon_button(BUTTON, "copy", "Copy file contents")])
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click
			|| event.point != mount::ATTACHMENT_ACTION
			|| event.node != BUTTON
		{
			return Reply::none();
		}
		let (Some(message), Some(attachment_id)) = (
			message_ref(&event.context),
			ui::context_detail().and_then(|detail| detail.attachment_id),
		) else {
			return Reply::none();
		};
		match copy(message, attachment_id) {
			Ok(notice) => Reply::none().toast(notice),
			Err(error) => Reply::none().toast(describe(&error)),
		}
	}
}

export_plugin!(CopyFileContents, ext);
