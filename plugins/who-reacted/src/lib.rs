use std::collections::{HashMap, VecDeque};

use plugin_sdk::model::{
	ButtonVariant, EmojiRef, MessageFull, Space, TextSize, UiContext, ViewEvent, ViewEventKind,
};
use plugin_sdk::patch::Patch;
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	Event, HostError, Md, MessageRef, Plugin, PluginExt, Reply, User, actions, client, data, export_plugin,
	log, menu, mount, storage, ui,
};

const CLICKABLE_KEY: &str = "clickableAvatars";
const ACCESSORY_POINT: &str = "message-accessory";
const BUTTON: &str = "reactors";
const HIDE_BUTTON: &str = "hide";
const MENU_ITEM: &str = "who";
const USERS_PER_EMOJI: u32 = 10;
const MAX_MESSAGES: usize = 100;
const AVATAR_SIZE: u32 = 20;
const EMOJI_SIZE: u32 = 18;
const TOAST_MS: u32 = 3000;

struct Group {
	emoji: EmojiRef,
	count: u32,
	users: Vec<User>,
}

struct WhoReacted {
	clickable: bool,
	reactors: HashMap<u64, Vec<Group>>,
	order: VecDeque<u64>,
}

impl Default for WhoReacted {
	fn default() -> Self {
		Self {
			clickable: true,
			reactors: HashMap::new(),
			order: VecDeque::new(),
		}
	}
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

fn message_ref(context: &UiContext) -> Option<MessageRef> {
	Some(MessageRef {
		id: context.message_id?,
		channel_id: context.channel_id?,
		guild_id: context.guild_id,
	})
}

fn row_of(group: &Group, clickable: bool) -> Node {
	let mut row = view::row()
		.gap(Space::Xs)
		.wrap()
		.child(view::emoji(group.emoji.clone(), EMOJI_SIZE));
	for user in &group.users {
		row = row.child(if clickable {
			view::user_chip(user.id)
		} else {
			view::avatar(user.id, AVATAR_SIZE)
		});
	}
	let hidden = (group.count as usize).saturating_sub(group.users.len());
	if hidden > 0 {
		row = row.child(view::text(format!("+{hidden}")).size(TextSize::Xs).muted());
	}
	row
}

impl WhoReacted {
	fn remember(&mut self, message_id: u64) -> &mut Vec<Group> {
		if !self.reactors.contains_key(&message_id) {
			self.order.push_back(message_id);
			if self.order.len() > MAX_MESSAGES
				&& let Some(oldest) = self.order.pop_front()
			{
				self.reactors.remove(&oldest);
			}
		}
		self.reactors.entry(message_id).or_default()
	}

	fn load(&mut self, message: MessageRef, only: Option<&EmojiRef>) -> Result<Option<String>, String> {
		let full = client::get_message(message.channel_id, message.id)
			.map_err(|error| describe(&error))?
			.ok_or("That message is not available")?;
		let wanted: Vec<_> = full
			.reactions
			.into_iter()
			.filter(|reaction| only.is_none_or(|emoji| *emoji == reaction.emoji))
			.collect();
		if wanted.is_empty() {
			return Err("That message has no reactions".to_owned());
		}
		let mut notice = None;
		for reaction in wanted {
			if self
				.reactors
				.get(&message.id)
				.is_some_and(|groups| groups.iter().any(|group| group.emoji == reaction.emoji))
			{
				continue;
			}
			match data::reaction_users(message, &reaction.emoji, USERS_PER_EMOJI) {
				Ok(users) => self.remember(message.id).push(Group {
					emoji: reaction.emoji,
					count: reaction.count,
					users,
				}),
				Err(HostError::RateLimited) => {
					notice = Some("Rate limited, click again for the rest".to_owned());
					break;
				}
				Err(error) => return Err(describe(&error)),
			}
		}
		actions::invalidate_message(message).map_err(|error| describe(&error))?;
		Ok(notice)
	}

	fn run(&mut self, message: Option<MessageRef>, only: Option<&EmojiRef>) -> Option<String> {
		let Some(message) = message else {
			return Some("Nothing to load".to_owned());
		};
		self.load(message, only).unwrap_or_else(Some)
	}

	fn hide(&mut self, message_id: u64) {
		if self.reactors.remove(&message_id).is_none() {
			return;
		}
		self.order.retain(|id| *id != message_id);
	}
}

impl Plugin for WhoReacted {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{CLICKABLE_KEY}")) {
			self.clickable = !matches!(String::from_utf8_lossy(&saved).trim(), "false" | "0" | "off");
		}
		let added = mount::mount(
			mount::MESSAGE_ACTION,
			BUTTON,
			[view::icon_button(BUTTON, "users", "Reactors")],
			mount::Options::new("Reactors"),
		)
		.and_then(|()| {
			menu::set(
				menu::REACTION,
				[menu::item(MENU_ITEM, "Who reacted").icon("users")],
			)
		});
		if let Err(error) = added {
			log::warn!("could not add the reactors controls: {error:?}");
		}
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == CLICKABLE_KEY
		{
			self.clickable = !matches!(value.trim(), "false" | "0" | "off");
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"clickableAvatars":{"type":"boolean","title":"Clickable users","description":"Show the reacting users as chips that open their profile when clicked, instead of plain avatars","default":true}}}"#
			.to_owned()
	}
}

impl PluginExt for WhoReacted {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind != ViewEventKind::Click {
			return Reply::none();
		}
		match (event.point.as_str(), event.node.as_str()) {
			(mount::MESSAGE_ACTION, BUTTON) => match self.run(message_ref(&event.context), None) {
				Some(notice) => Reply::none().toast(notice),
				None => Reply::none(),
			},
			(ACCESSORY_POINT, HIDE_BUTTON) => {
				if let Ok(message_id) = event.mount.parse() {
					self.hide(message_id);
					if let Some(message) = message_ref(&event.context)
						&& let Err(error) = actions::invalidate_message(message)
					{
						log::debug!("could not refresh message {message_id}: {error:?}");
					}
				}
				Reply::none()
			}
			_ => Reply::none(),
		}
	}

	fn on_menu_item(&mut self, menu: &str, key: &str, context: &UiContext) {
		if menu != menu::REACTION || key != MENU_ITEM {
			return;
		}
		let emoji = ui::context_detail().and_then(|detail| detail.reaction);
		if let Some(notice) = self.run(message_ref(context), emoji.as_ref())
			&& let Err(error) = mount::toast([view::text(notice)], TOAST_MS)
		{
			log::warn!("could not show the toast: {error:?}");
		}
	}

	fn intercept_message(&mut self, msg: &MessageFull, _content: Vec<Md>) -> Patch {
		match self.reactors.get(&msg.view.id) {
			Some(groups) if !groups.is_empty() => {
				let mut column = view::column().gap(Space::Xs);
				for group in groups {
					column = column.child(row_of(group, self.clickable));
				}
				Patch::keep()
					.accessory([column.child(view::button(HIDE_BUTTON, "Hide").variant(ButtonVariant::Link))])
			}
			_ => Patch::keep(),
		}
	}
}

export_plugin!(WhoReacted, ext);
