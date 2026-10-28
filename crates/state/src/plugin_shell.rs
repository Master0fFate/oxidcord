use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use protocol::Snowflake;
use serde::{Deserialize, Serialize};

use crate::state::State;

const INTENT_BACKLOG: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub enum UiIntent {
	StartReply {
		channel_id: Snowflake,
		message_id: Snowflake,
		mention: bool,
	},
	StartEdit {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	SetComposerText {
		channel_id: Snowflake,
		content: String,
	},
	AttachFiles {
		channel_id: Snowflake,
		paths: Vec<PathBuf>,
	},
	AttachSticker {
		channel_id: Snowflake,
		sticker_id: Snowflake,
	},
	JumpToMessage {
		channel_id: Snowflake,
		message_id: Snowflake,
	},
	JumpToTop {
		channel_id: Snowflake,
	},
	OpenSettings(String),
	NavigateBack,
	NavigateForward,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueuedIntent {
	pub id: u64,
	pub intent: UiIntent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientOptions {
	pub message_toolbar_expanded: Option<bool>,
	pub plain_folder_icons: Option<bool>,
	pub folder_sidebar: Option<bool>,
	pub forum_layout: Option<u8>,
	pub forum_sort: Option<u8>,
	pub notification_volume: Option<u8>,
	pub streamer_mode: Option<bool>,
	pub typing_animation: Option<bool>,
	pub super_react_default: Option<bool>,
	pub reply_ping_default: Option<bool>,
}

impl ClientOptions {
	pub fn overlay(&mut self, over: &ClientOptions) {
		self.message_toolbar_expanded = over.message_toolbar_expanded.or(self.message_toolbar_expanded);
		self.plain_folder_icons = over.plain_folder_icons.or(self.plain_folder_icons);
		self.folder_sidebar = over.folder_sidebar.or(self.folder_sidebar);
		self.forum_layout = over.forum_layout.or(self.forum_layout);
		self.forum_sort = over.forum_sort.or(self.forum_sort);
		self.notification_volume = over.notification_volume.or(self.notification_volume);
		self.streamer_mode = over.streamer_mode.or(self.streamer_mode);
		self.typing_animation = over.typing_animation.or(self.typing_animation);
		self.super_react_default = over.super_react_default.or(self.super_react_default);
		self.reply_ping_default = over.reply_ping_default.or(self.reply_ping_default);
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmSectionView {
	pub plugin_id: String,
	pub title: String,
	pub channels: Vec<Snowflake>,
	pub collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadgeColor {
	Token(String),
	Rgba(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBadge {
	pub icon: Option<String>,
	pub image: Option<String>,
	pub label: Option<String>,
	pub tooltip: String,
	pub color: Option<BadgeColor>,
	pub key: Option<String>,
	pub plugin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocompleteOrder {
	pub generation: u64,
	pub order: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelDecoration {
	pub plugin_id: String,
	pub badges: Vec<PluginBadge>,
}

#[derive(Debug, Default)]
pub(crate) struct PluginShell {
	pub(crate) intents: VecDeque<QueuedIntent>,
	pub(crate) intent_count: u64,
	pub(crate) client_options: ClientOptions,
	pub(crate) dm_sections: Vec<DmSectionView>,
	pub(crate) autocomplete: Option<AutocompleteOrder>,
	pub(crate) decorations: HashMap<Snowflake, Vec<ChannelDecoration>>,
	pub(crate) hold_alerts: bool,
}

impl PluginShell {
	pub(crate) fn push_intent(&mut self, intent: UiIntent) {
		if self.intents.len() == INTENT_BACKLOG {
			self.intents.pop_front();
		}
		self.intent_count += 1;
		self.intents.push_back(QueuedIntent {
			id: self.intent_count,
			intent,
		});
	}
}

impl State {
	pub fn intents_after(&self, seen: u64) -> impl Iterator<Item = &QueuedIntent> {
		self.plugin_shell
			.intents
			.iter()
			.filter(move |queued| queued.id > seen)
	}

	pub fn last_intent_id(&self) -> u64 {
		self.plugin_shell.intent_count
	}

	pub fn client_options(&self) -> &ClientOptions {
		&self.plugin_shell.client_options
	}

	pub fn dm_sections(&self) -> &[DmSectionView] {
		&self.plugin_shell.dm_sections
	}

	pub fn autocomplete_order(&self, generation: u64) -> Option<&[u32]> {
		self.plugin_shell
			.autocomplete
			.as_ref()
			.filter(|answer| answer.generation == generation)
			.map(|answer| answer.order.as_slice())
	}

	pub fn channel_decorations(&self, channel_id: Snowflake) -> &[ChannelDecoration] {
		self.plugin_shell
			.decorations
			.get(&channel_id)
			.map_or(&[], Vec::as_slice)
	}
}
