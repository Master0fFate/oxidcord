use plugin_sdk::model::{Align, ClientEvent, Relationship, RelationshipKind, Space};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Plugin, PluginExt, data, export_plugin, log, mount, timer};

const MOUNT_KEY: &str = "implicit";
const REFRESH_TAG: &str = "refresh";
const REFRESH_MS: u32 = 60_000;
const DEBOUNCE_MS: u32 = 1_000;
const MAX_ROWS: usize = 300;
const AVATAR_SIZE: u32 = 32;

#[derive(Default)]
struct ImplicitRelationships {
	refresh_queued: bool,
}

impl ImplicitRelationships {
	fn refresh(&mut self) {
		self.refresh_queued = false;
		let people = match data::list_relationships() {
			Ok(people) => people,
			Err(error) => {
				log::warn!("could not read relationships: {error:?}");
				return;
			}
		};
		if let Err(error) = mount::update(mount::FRIENDS_SECTION, MOUNT_KEY, page(people)) {
			log::warn!("could not update the implicit tab: {error:?}");
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
}

fn page(mut people: Vec<Relationship>) -> Vec<Node> {
	people.retain(|relationship| relationship.kind == RelationshipKind::Implicit);
	people.sort_by_cached_key(|relationship| {
		(
			relationship.user.display_name.to_lowercase(),
			relationship.user.username.to_lowercase(),
		)
	});
	let summary = match people.len() {
		0 => "Nobody yet".to_owned(),
		count => format!("{count} people"),
	};
	vec![
		view::column()
			.gap(Space::Sm)
			.child(view::text(summary).muted())
			.children(people.into_iter().take(MAX_ROWS).map(row)),
	]
}

fn row(relationship: Relationship) -> Node {
	let user = relationship.user;
	view::row()
		.gap(Space::Md)
		.align(Align::Center)
		.child(view::avatar(user.id, AVATAR_SIZE))
		.child(view::text(user.display_name).bold())
		.child(view::text(user.username).muted())
}

impl Plugin for ImplicitRelationships {
	fn init(&mut self) -> Result<(), String> {
		let options = mount::Options::new("Implicit").icon("users");
		if let Err(error) = mount::mount(mount::FRIENDS_SECTION, MOUNT_KEY, page(Vec::new()), options) {
			log::warn!("could not mount the implicit tab: {error:?}");
		}
		if let Err(error) = timer::every(REFRESH_MS, REFRESH_TAG) {
			log::warn!("could not start the refresh timer: {error:?}");
		}
		self.refresh();
		Ok(())
	}
}

impl PluginExt for ImplicitRelationships {
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

export_plugin!(ImplicitRelationships, ext);
