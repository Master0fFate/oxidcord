use std::cmp::Reverse;

use plugin_sdk::model::{DmSection, Radius, Space, Tone, UiContext, ViewEvent, ViewEventKind};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{
	ChannelSummary, Color, Event, Plugin, PluginExt, Reply, actions, data, export_plugin, host, log, menu,
	mount, storage, timer,
};

const CATEGORIES_KEY: &str = "categories";
const ORDER_KEY: &str = "pinOrder";
const COLLAPSED_KEY: &str = "startCollapsed";
const MOUNT_KEY: &str = "categories";
const PIN_PARENT: &str = "pin";
const PIN_PREFIX: &str = "pin-";
const UNPIN: &str = "unpin";
const MOVE_UP: &str = "up";
const MOVE_DOWN: &str = "down";
const ADD_FORM: &str = "add-category";
const NAME_FIELD: &str = "name";
const DELETE_PREFIX: &str = "delete-";
const REMOVE_PREFIX: &str = "remove-";
const PUT_PREFIX: &str = "put-";
const SORT_TAG: &str = "sort";
const SORT_DELAY_MS: u32 = 2_000;
const DEFAULT_CATEGORY: &str = "Pinned";
const MAX_CATEGORIES: usize = 8;
const MAX_NAME_CHARS: usize = 32;
const MAX_OPTIONS: usize = 50;

#[derive(Clone, PartialEq)]
struct Category {
	id: u32,
	name: String,
	channels: Vec<u64>,
}

#[derive(Default)]
struct Config {
	custom_order: bool,
	collapsed: bool,
}

fn parse_flag(raw: &str) -> bool {
	matches!(raw.trim(), "true" | "1" | "on")
}

impl Config {
	fn set(&mut self, key: &str, value: &str) -> bool {
		match key {
			ORDER_KEY => self.custom_order = value.trim() == "custom",
			COLLAPSED_KEY => self.collapsed = parse_flag(value),
			_ => return false,
		}
		true
	}
}

fn encode(categories: &[Category]) -> String {
	categories
		.iter()
		.map(|category| {
			let channels: Vec<String> = category.channels.iter().map(u64::to_string).collect();
			format!("{}\t{}\t{}\n", category.id, channels.join(","), category.name)
		})
		.collect()
}

fn decode(raw: &str) -> Vec<Category> {
	raw.lines()
		.filter_map(|line| {
			let mut parts = line.splitn(3, '\t');
			let id = parts.next()?.parse().ok()?;
			let channels = parts
				.next()?
				.split(',')
				.filter_map(|channel| channel.parse().ok())
				.collect();
			let name = parts.next()?.to_owned();
			Some(Category { id, name, channels })
		})
		.collect()
}

fn clean_name(raw: &str) -> String {
	raw.chars()
		.filter(|letter| !letter.is_control())
		.take(MAX_NAME_CHARS)
		.collect::<String>()
		.trim()
		.to_owned()
}

fn last_message(channel_id: u64) -> u64 {
	data::channel_detail(channel_id)
		.ok()
		.flatten()
		.and_then(|detail| detail.last_message_id)
		.unwrap_or_default()
}

#[derive(Default)]
struct PinDms {
	config: Config,
	categories: Vec<Category>,
	sort_timer: Option<u64>,
}

impl PinDms {
	fn pinned(&self, channel_id: u64) -> bool {
		self.categories
			.iter()
			.any(|category| category.channels.contains(&channel_id))
	}

	fn ensure_default(&mut self) {
		if self.categories.is_empty() {
			self.categories.push(Category {
				id: 1,
				name: DEFAULT_CATEGORY.to_owned(),
				channels: Vec::new(),
			});
		}
	}

	fn unpin(&mut self, channel_id: u64) {
		for category in &mut self.categories {
			category.channels.retain(|pinned| *pinned != channel_id);
		}
	}

	fn pin(&mut self, channel_id: u64, category_id: u32) {
		self.unpin(channel_id);
		if let Some(category) = self
			.categories
			.iter_mut()
			.find(|category| category.id == category_id)
		{
			category.channels.push(channel_id);
		}
	}

	fn shift(&mut self, channel_id: u64, down: bool) {
		let Some(category) = self
			.categories
			.iter_mut()
			.find(|category| category.channels.contains(&channel_id))
		else {
			return;
		};
		let Some(index) = category.channels.iter().position(|pinned| *pinned == channel_id) else {
			return;
		};
		let other = if down { index + 1 } else { index.wrapping_sub(1) };
		if other < category.channels.len() {
			category.channels.swap(index, other);
		}
	}

	fn add_category(&mut self, name: &str) -> Result<(), &'static str> {
		let name = clean_name(name);
		if name.is_empty() {
			return Err("Name the category first");
		}
		if self.categories.len() >= MAX_CATEGORIES {
			return Err("That is the most categories");
		}
		if self
			.categories
			.iter()
			.any(|category| category.name.eq_ignore_ascii_case(&name))
		{
			return Err("That name is taken");
		}
		let id = self
			.categories
			.iter()
			.map(|category| category.id)
			.max()
			.unwrap_or(0)
			+ 1;
		self.categories.push(Category {
			id,
			name,
			channels: Vec::new(),
		});
		Ok(())
	}

	fn sections(&self) -> Vec<DmSection> {
		self.categories
			.iter()
			.filter(|category| !category.channels.is_empty())
			.map(|category| {
				let mut channels = category.channels.clone();
				if !self.config.custom_order {
					channels.sort_by_cached_key(|channel| Reverse(last_message(*channel)));
				}
				DmSection {
					title: category.name.clone(),
					channels,
					collapsed: self.config.collapsed,
				}
			})
			.collect()
	}

	fn sync_sections(&self) {
		if let Err(error) = actions::set_dm_sections(&self.sections()) {
			log::warn!("could not set the pinned sections: {error:?}");
		}
	}

	fn sync_menu(&self) {
		let in_dm = menu::When::IN_DM;
		let mut items = vec![menu::item(PIN_PARENT, "Pin DM").icon("pin").when(in_dm)];
		items.extend(self.categories.iter().map(|category| {
			menu::item(format!("{PIN_PREFIX}{}", category.id), category.name.clone())
				.parent(PIN_PARENT)
				.when(in_dm)
		}));
		items.push(menu::item(UNPIN, "Unpin DM").tone(Tone::Danger).when(in_dm));
		if self.config.custom_order {
			items.push(menu::item(MOVE_UP, "Move pin up").when(in_dm));
			items.push(menu::item(MOVE_DOWN, "Move pin down").when(in_dm));
		}
		if let Err(error) = menu::set(menu::CHANNEL, items) {
			log::warn!("could not set the DM menu: {error:?}");
		}
	}

	fn sync_page(&self) {
		if let Err(error) = mount::update(mount::SETTINGS_PAGE, MOUNT_KEY, self.page()) {
			log::debug!("could not update the pinned DMs page: {error:?}");
		}
	}

	fn changed(&self) {
		if let Err(error) = storage::set(CATEGORIES_KEY, encode(&self.categories).as_bytes()) {
			log::warn!("could not save the pinned DMs: {error:?}");
		}
		self.sync_sections();
		self.sync_menu();
		self.sync_page();
	}

	fn schedule_sort(&mut self) {
		if self.sort_timer.is_some() {
			return;
		}
		match timer::once(SORT_DELAY_MS, SORT_TAG) {
			Ok(id) => self.sort_timer = Some(id),
			Err(error) => log::warn!("could not schedule the reorder: {error:?}"),
		}
	}

	fn card(&self, category: &Category, free: &[ChannelSummary]) -> Node {
		let mut card = view::column()
			.gap(Space::Sm)
			.padding(Space::Md)
			.background(Color::token("background-secondary"))
			.radius(Radius::Md)
			.child(
				view::row()
					.gap(Space::Sm)
					.child(view::text(category.name.clone()).bold())
					.child(view::spacer())
					.child(view::icon_button(
						format!("{DELETE_PREFIX}{}", category.id),
						"trash",
						"Delete category",
					)),
			);
		for channel in &category.channels {
			card = card.child(
				view::row()
					.gap(Space::Sm)
					.child(view::channel_chip(*channel))
					.child(view::spacer())
					.child(view::icon_button(
						format!("{REMOVE_PREFIX}{}-{channel}", category.id),
						"x",
						"Unpin",
					)),
			);
		}
		let mut select = view::select(format!("{PUT_PREFIX}{}", category.id), "Add a DM");
		for dm in free.iter().take(MAX_OPTIONS) {
			select = select.option(dm.id.to_string(), dm.name.clone());
		}
		card.child(select)
	}

	fn page(&self) -> Vec<Node> {
		let free: Vec<ChannelSummary> = host::list_channels(None)
			.unwrap_or_default()
			.into_iter()
			.filter(|dm| !self.pinned(dm.id))
			.collect();
		let mut column = view::column()
			.gap(Space::Md)
			.padding(Space::Md)
			.child(view::heading("Pinned DMs"))
			.child(
				view::form().key(ADD_FORM).child(
					view::row()
						.gap(Space::Sm)
						.child(view::input(NAME_FIELD, "New category"))
						.child(view::button("add", "Add category").submit()),
				),
			);
		for category in &self.categories {
			column = column.child(self.card(category, &free));
		}
		vec![column]
	}

	fn click(&mut self, node: &str) -> bool {
		if let Some(id) = node
			.strip_prefix(DELETE_PREFIX)
			.and_then(|id| id.parse::<u32>().ok())
		{
			self.categories.retain(|category| category.id != id);
			self.ensure_default();
			return true;
		}
		let Some((category_id, channel_id)) = node
			.strip_prefix(REMOVE_PREFIX)
			.and_then(|ids| ids.split_once('-'))
			.and_then(|(category, channel)| {
				Some((category.parse::<u32>().ok()?, channel.parse::<u64>().ok()?))
			})
		else {
			return false;
		};
		if let Some(category) = self
			.categories
			.iter_mut()
			.find(|category| category.id == category_id)
		{
			category.channels.retain(|pinned| *pinned != channel_id);
		}
		true
	}
}

impl Plugin for PinDms {
	fn init(&mut self) -> Result<(), String> {
		for key in [ORDER_KEY, COLLAPSED_KEY] {
			if let Ok(Some(saved)) = storage::get(&format!("settings.{key}")) {
				self.config.set(key, &String::from_utf8_lossy(&saved));
			}
		}
		if let Ok(Some(saved)) = storage::get(CATEGORIES_KEY) {
			self.categories = decode(&String::from_utf8_lossy(&saved));
		}
		self.ensure_default();
		self.sync_menu();
		let options = mount::Options::new("Pinned DMs").icon("pin");
		if let Err(error) = mount::mount(mount::SETTINGS_PAGE, MOUNT_KEY, self.page(), options) {
			log::warn!("could not mount the pinned DMs page: {error:?}");
		}
		self.sync_sections();
		self.schedule_sort();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		match event {
			Event::MessageCreated(message)
				if !self.config.custom_order && self.pinned(message.channel_id) =>
			{
				self.schedule_sort();
			}
			Event::SettingChanged(SettingChange { key, value }) if self.config.set(&key, &value) => {
				self.sync_sections();
				self.sync_menu();
			}
			_ => {}
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"pinOrder":{"type":"string","title":"Pin order","description":"Which order should pinned DMs be displayed in? mostRecent sorts by the latest message, custom keeps the order you set with Move pin up and down in the DM menu","enum":["mostRecent","custom"],"default":"mostRecent"},"startCollapsed":{"type":"boolean","title":"Start collapsed","description":"Start the pinned sections collapsed, you can still open them by clicking the header","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for PinDms {
	fn on_menu_item(&mut self, menu_name: &str, key: &str, context: &UiContext) {
		let Some(channel_id) = context.channel_id.filter(|_| menu_name == menu::CHANNEL) else {
			return;
		};
		match key {
			UNPIN => self.unpin(channel_id),
			MOVE_UP => self.shift(channel_id, false),
			MOVE_DOWN => self.shift(channel_id, true),
			_ => match key.strip_prefix(PIN_PREFIX).and_then(|id| id.parse().ok()) {
				Some(category_id) => self.pin(channel_id, category_id),
				None => return,
			},
		}
		self.changed();
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == SORT_TAG {
			self.sort_timer = None;
			self.sync_sections();
		}
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		let changed = match (&event.kind, event.node.as_str()) {
			(ViewEventKind::Submit(values), ADD_FORM) => {
				let name = values
					.iter()
					.find(|(field, _)| field == NAME_FIELD)
					.map_or("", |(_, value)| value.as_str());
				match self.add_category(name) {
					Ok(()) => true,
					Err(reason) => return Reply::none().toast(reason),
				}
			}
			(ViewEventKind::Click, node) => self.click(node),
			(ViewEventKind::Select(value), node) => node
				.strip_prefix(PUT_PREFIX)
				.and_then(|id| id.parse().ok())
				.zip(value.parse().ok())
				.map(|(category_id, channel_id)| self.pin(channel_id, category_id))
				.is_some(),
			_ => false,
		};
		if changed {
			self.changed();
		}
		Reply::none()
	}
}

export_plugin!(PinDms, ext);
