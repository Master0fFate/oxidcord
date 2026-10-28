use plugin_sdk::model::{
	Align, ButtonVariant, ClientEvent, Space, TextSize, UiContext, ViewEvent, ViewEventKind,
};
use plugin_sdk::view::{self, Node};
use plugin_sdk::{Plugin, PluginExt, Reply, data, export_plugin, host, log, mount};

const GROUP_DM_KIND: u32 = 3;
const OPEN_PREFIX: &str = "open:";
const MAX_GROUPS: usize = 30;

#[derive(Default)]
struct MutualGroupDms;

struct Group {
	id: u64,
	name: String,
	members: usize,
}

fn shared_groups(user_id: u64) -> Vec<Group> {
	let channels = match host::list_channels(None) {
		Ok(channels) => channels,
		Err(error) => {
			log::warn!("could not list group chats: {error:?}");
			return Vec::new();
		}
	};
	channels
		.into_iter()
		.filter(|channel| channel.kind == GROUP_DM_KIND)
		.filter_map(|channel| {
			let detail = data::channel_detail(channel.id).ok().flatten()?;
			detail.recipients.contains(&user_id).then(|| Group {
				id: channel.id,
				name: channel.name,
				members: detail.recipients.len() + 1,
			})
		})
		.take(MAX_GROUPS)
		.collect()
}

fn row(group: Group) -> Node {
	view::row()
		.gap(Space::Sm)
		.align(Align::Center)
		.child(
			view::button(format!("{OPEN_PREFIX}{}", group.id), group.name)
				.icon("users")
				.variant(ButtonVariant::Link),
		)
		.child(
			view::text(format!("{} members", group.members))
				.size(TextSize::Xs)
				.muted(),
		)
}

impl Plugin for MutualGroupDms {
	fn init(&mut self) -> Result<(), String> {
		if let Err(error) = mount::provide(mount::USER_POPOUT_SECTION, mount::Options::new("Mutual groups")) {
			log::warn!("could not provide the popout section: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for MutualGroupDms {
	fn render_view(&mut self, point: &str, context: &UiContext) -> Option<Vec<Node>> {
		if point != mount::USER_POPOUT_SECTION {
			return None;
		}
		let groups = shared_groups(context.user_id?);
		if groups.is_empty() {
			return None;
		}
		let title = format!("{} mutual groups", groups.len());
		Some(vec![
			view::section(title).child(
				view::column()
					.gap(Space::Xs)
					.children(groups.into_iter().map(row)),
			),
		])
	}

	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.kind == ViewEventKind::Click
			&& let Some(id) = event
				.node
				.strip_prefix(OPEN_PREFIX)
				.and_then(|id| id.parse().ok())
			&& let Err(error) = host::open_channel(None, id)
		{
			log::warn!("could not open the group chat: {error:?}");
		}
		Reply::none()
	}

	fn on_client_event(&mut self, event: ClientEvent) {
		if matches!(event, ClientEvent::DmClosed(_) | ClientEvent::ChannelListChanged)
			&& let Err(error) = mount::invalidate(mount::USER_POPOUT_SECTION)
		{
			log::debug!("could not refresh the popout section: {error:?}");
		}
	}
}

export_plugin!(MutualGroupDms, ext);
