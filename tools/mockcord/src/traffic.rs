use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use protocol::{EditMessage, StatusKind, User};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::fixtures::corpus::{SENTENCES, pick};
use crate::fixtures::{CURRENT_USER_ID, random_presence};
use crate::json::parse_id;
use crate::member_list::presence_json;
use crate::ops::Draft;
use crate::state::Shared;

const CHAOS_CLOSE_INTERVAL: Duration = Duration::from_secs(90);
const CHAOS_CLOSE_CODE: u16 = 4000;
const MESSAGE_INTERVAL: Duration = Duration::from_secs(3);
const TYPING_LEAD: Duration = Duration::from_secs(1);
const PRESENCE_INTERVAL: Duration = Duration::from_secs(10);
const RECENT_CAPACITY: usize = 6;
const EDIT_EVERY: u64 = 3;
const DELETE_EVERY: u64 = 4;
const STATUSES: [StatusKind; 4] = [
	StatusKind::Online,
	StatusKind::Idle,
	StatusKind::Dnd,
	StatusKind::Offline,
];

pub fn spawn(shared: &Arc<Shared>) -> Vec<JoinHandle<()>> {
	let mut tasks = Vec::new();
	if shared.config.chaos {
		tasks.push(tokio::spawn(chaos_loop(shared.clone())));
	}
	if shared.config.live_traffic {
		tasks.push(tokio::spawn(message_loop(shared.clone())));
		tasks.push(tokio::spawn(presence_loop(shared.clone())));
	}
	tasks
}

fn live_rng() -> StdRng {
	StdRng::from_rng(&mut rand::rng())
}

async fn interrupted(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
	tokio::select! {
		() = tokio::time::sleep(duration) => false,
		_ = shutdown.changed() => true,
	}
}

async fn chaos_loop(shared: Arc<Shared>) {
	let mut shutdown = shared.shutdown.clone();
	while !interrupted(&mut shutdown, CHAOS_CLOSE_INTERVAL).await {
		tracing::info!(code = CHAOS_CLOSE_CODE, "chaos: closing gateway connections");
		shared.hub.close_all(CHAOS_CLOSE_CODE);
	}
}

struct Target {
	channel_id: u64,
	author: User,
	content: String,
}

fn pick_target(shared: &Shared) -> Option<Target> {
	let fixtures = &shared.fixtures;
	let mut rng = live_rng();
	let total: usize = fixtures
		.guilds
		.iter()
		.map(|guild| guild.message_channel_ids.len())
		.sum::<usize>()
		+ fixtures.private_channel_ids.len();
	if total == 0 {
		return None;
	}
	let mut index = rng.random_range(0..total);
	let mut resolved = None;
	for guild in &fixtures.guilds {
		if index < guild.message_channel_ids.len() {
			let authors: Vec<&User> = guild
				.members
				.iter()
				.skip(1)
				.filter_map(|member| member.user.as_ref())
				.collect();
			resolved = Some((guild.message_channel_ids[index], authors));
			break;
		}
		index -= guild.message_channel_ids.len();
	}
	let (channel_id, authors) = match resolved {
		Some(found) => found,
		None => {
			let channel_id = fixtures.private_channel_ids[index];
			let channel = fixtures.channels.get(&channel_id)?;
			(channel_id, channel.recipients.iter().collect())
		}
	};
	if authors.is_empty() {
		return None;
	}
	let author = (*pick(&mut rng, &authors)).clone();
	let sentence = *pick(&mut rng, SENTENCES);
	let content = if rng.random_bool(0.15) {
		format!("<@{CURRENT_USER_ID}> {sentence}")
	} else {
		sentence.to_owned()
	};
	Some(Target {
		channel_id,
		author,
		content,
	})
}

struct Posted {
	channel_id: u64,
	id: u64,
	author_id: u64,
}

async fn message_loop(shared: Arc<Shared>) {
	let mut shutdown = shared.shutdown.clone();
	let mut recent: VecDeque<Posted> = VecDeque::new();
	let mut round: u64 = 0;
	loop {
		if interrupted(&mut shutdown, MESSAGE_INTERVAL - TYPING_LEAD).await {
			return;
		}
		let Some(target) = pick_target(&shared) else {
			continue;
		};
		if shared.typing(target.channel_id, target.author.id.0).is_err() {
			continue;
		}
		if interrupted(&mut shutdown, TYPING_LEAD).await {
			return;
		}
		let posted = shared.post_message(target.channel_id, &target.author, Draft::text(target.content));
		if let Some(id) = posted.ok().and_then(|value| parse_id(&value["id"])) {
			tracing::info!(channel_id = target.channel_id, "live message");
			recent.push_back(Posted {
				channel_id: target.channel_id,
				id,
				author_id: target.author.id.0,
			});
			if recent.len() > RECENT_CAPACITY {
				recent.pop_front();
			}
		}
		round += 1;
		if round.is_multiple_of(EDIT_EVERY) {
			edit_recent(&shared, &recent);
		}
		if round.is_multiple_of(DELETE_EVERY) {
			delete_oldest(&shared, &mut recent);
		}
	}
}

fn edit_recent(shared: &Shared, recent: &VecDeque<Posted>) {
	let Some(posted) = recent.iter().rev().nth(1) else {
		return;
	};
	let edit = EditMessage {
		content: Some((*pick(&mut live_rng(), SENTENCES)).to_owned()),
		..EditMessage::default()
	};
	if shared
		.edit_as(posted.channel_id, posted.id, posted.author_id, edit)
		.is_ok()
	{
		tracing::info!(channel_id = posted.channel_id, "live edit");
	}
}

fn delete_oldest(shared: &Shared, recent: &mut VecDeque<Posted>) {
	if recent.len() < RECENT_CAPACITY {
		return;
	}
	if let Some(posted) = recent.pop_front()
		&& shared.delete_message(posted.channel_id, posted.id).is_ok()
	{
		tracing::info!(channel_id = posted.channel_id, "live delete");
	}
}

fn change_presence(shared: &Shared) {
	let mut rng = live_rng();
	let guilds = &shared.fixtures.guilds;
	if guilds.is_empty() {
		return;
	}
	let guild = &guilds[rng.random_range(0..guilds.len())];
	if guild.members.len() < 2 {
		return;
	}
	let member = &guild.members[rng.random_range(1..guild.members.len())];
	let Some(user) = &member.user else {
		return;
	};
	let status = STATUSES[rng.random_range(0..STATUSES.len())].clone();
	let presence = random_presence(&mut rng, user.id.0, status);
	{
		let mut store = shared.store.lock();
		let by_user = store.presences.entry(guild.id).or_default();
		if presence.status == StatusKind::Offline {
			by_user.remove(&user.id.0);
		} else {
			by_user.insert(user.id.0, presence.clone());
		}
	}
	shared
		.hub
		.broadcast("PRESENCE_UPDATE", &presence_json(guild.id, user.id.0, &presence));
}

async fn presence_loop(shared: Arc<Shared>) {
	let mut shutdown = shared.shutdown.clone();
	while !interrupted(&mut shutdown, PRESENCE_INTERVAL).await {
		change_presence(&shared);
	}
}
