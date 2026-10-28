use std::cmp::Reverse;

use plugin_sdk::model::{Align, ClientEvent, Relationship, RelationshipKind, Space};
use plugin_sdk::types::SettingChange;
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Event, Plugin, PluginExt, data, export_plugin, log, mount, storage, timer};

const MOUNT_KEY: &str = "requests";
const REFRESH_TAG: &str = "refresh";
const DEBOUNCE_MS: u32 = 1_000;
const SHOW_DATES_KEY: &str = "showDates";
const AVATAR_SIZE: u32 = 32;

#[derive(Default)]
struct SortFriendRequests {
	show_dates: bool,
	refresh_queued: bool,
}

fn parse_flag(raw: &str) -> bool {
	matches!(raw.trim(), "true" | "1" | "on")
}

impl SortFriendRequests {
	fn refresh(&mut self) {
		self.refresh_queued = false;
		let people = match data::list_relationships() {
			Ok(people) => people,
			Err(error) => {
				log::warn!("could not read relationships: {error:?}");
				return;
			}
		};
		if let Err(error) = mount::update(mount::FRIENDS_SECTION, MOUNT_KEY, self.page(people)) {
			log::warn!("could not update the requests tab: {error:?}");
		}
	}

	fn queue_refresh(&mut self) {
		if self.refresh_queued {
			return;
		}
		match timer::once(DEBOUNCE_MS, REFRESH_TAG) {
			Ok(_) => self.refresh_queued = true,
			Err(error) => log::warn!("could not schedule a refresh: {error:?}"),
		}
	}

	fn page(&self, mut people: Vec<Relationship>) -> Vec<Node> {
		people.retain(|relationship| {
			matches!(
				relationship.kind,
				RelationshipKind::PendingIncoming | RelationshipKind::PendingOutgoing
			)
		});
		people.sort_by_key(|relationship| Reverse(relationship.since_ms));
		let summary = match people.len() {
			0 => "No requests".to_owned(),
			count => format!("{count} pending"),
		};
		vec![
			view::column()
				.gap(Space::Sm)
				.child(view::text(summary).muted())
				.children(people.into_iter().map(|relationship| self.row(relationship))),
		]
	}

	fn row(&self, relationship: Relationship) -> Node {
		let (icon, hint) = if relationship.kind == RelationshipKind::PendingIncoming {
			("arrow-down-left", "Incoming")
		} else {
			("arrow-up-right", "Outgoing")
		};
		let user = relationship.user;
		let mut row = view::row()
			.gap(Space::Md)
			.align(Align::Center)
			.child(view::icon(icon).tooltip(hint))
			.child(view::avatar(user.id, AVATAR_SIZE))
			.child(view::text(user.display_name).bold())
			.child(view::text(user.username).muted());
		if self.show_dates
			&& let Some(since_ms) = relationship.since_ms
		{
			row = row.child(view::spacer()).child(view::timestamp(since_ms, "d"));
		}
		row
	}
}

impl Plugin for SortFriendRequests {
	fn init(&mut self) -> Result<(), String> {
		if let Ok(Some(saved)) = storage::get(&format!("settings.{SHOW_DATES_KEY}")) {
			self.show_dates = parse_flag(&String::from_utf8_lossy(&saved));
		}
		let options = mount::Options::new("Requests by date").icon("user-plus");
		if let Err(error) = mount::mount(mount::FRIENDS_SECTION, MOUNT_KEY, Vec::<Node>::new(), options) {
			log::warn!("could not mount the requests tab: {error:?}");
		}
		self.refresh();
		Ok(())
	}

	fn on_event(&mut self, event: Event) {
		if let Event::SettingChanged(SettingChange { key, value }) = event
			&& key == SHOW_DATES_KEY
		{
			self.show_dates = parse_flag(&value);
			self.queue_refresh();
		}
	}

	fn settings_schema(&self) -> String {
		r#"{"type":"object","properties":{"showDates":{"type":"boolean","title":"Show dates","description":"Show dates on friend requests","default":false}}}"#
			.to_owned()
	}
}

impl PluginExt for SortFriendRequests {
	fn on_client_event(&mut self, event: ClientEvent) {
		if matches!(
			event,
			ClientEvent::RelationshipChanged(_) | ClientEvent::RelationshipRemoved(_) | ClientEvent::Ready
		) {
			self.queue_refresh();
		}
	}

	fn on_timer(&mut self, _id: u64, tag: &str) {
		if tag == REFRESH_TAG {
			self.refresh();
		}
	}
}

export_plugin!(SortFriendRequests, ext);
