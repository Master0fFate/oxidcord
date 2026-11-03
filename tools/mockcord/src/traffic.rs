use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use protocol::{EditMessage, MessageFlags, ScheduledEventStatus, StatusKind, User};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde_json::json;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::fixtures::corpus::{SENTENCES, pick};
use crate::fixtures::{CURRENT_USER_ID, GuildFixture, GuildOrigin, random_presence};
use crate::json::parse_id;
use crate::member_list::presence_json;
use crate::ops::Draft;
use crate::state::Shared;

const CHAOS_CLOSE_INTERVAL: Duration = Duration::from_secs(90);
const CHAOS_CLOSE_CODE: u16 = 4000;
const MESSAGE_INTERVAL: Duration = Duration::from_secs(3);
const TYPING_LEAD: Duration = Duration::from_secs(1);
const PRESENCE_INTERVAL: Duration = Duration::from_secs(10);
const EVENT_START_INTERVAL: Duration = Duration::from_secs(45);
const SOUND_INTERVAL: Duration = Duration::from_secs(30);
const RECENT_CAPACITY: usize = 6;
const EDIT_EVERY: u64 = 3;
const DELETE_EVERY: u64 = 4;
const THREAD_MENTION_EVERY: u64 = 5;
const SILENT_MENTION_CHANCE: f64 = 0.3;
const STORM_EVERY: u64 = 5;
const POLL_VOTE_EVERY: u64 = 4;
const STORM_USERS: std::ops::RangeInclusive<usize> = 3..=5;
const STORM_EMOJI: [&str; 4] = ["🔥", "🎉", "👍", "😂"];
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
		tasks.push(tokio::spawn(event_loop(shared.clone())));
		tasks.push(tokio::spawn(sound_loop(shared.clone())));
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
	flags: MessageFlags,
}

fn pick_target(shared: &Shared) -> Option<Target> {
	let fixtures = &shared.fixtures;
	let mut rng = live_rng();
	let guilds: Vec<&GuildFixture> = fixtures
		.guilds
		.iter()
		.filter(|guild| guild.origin == GuildOrigin::Seeded)
		.collect();
	let total: usize = guilds
		.iter()
		.map(|guild| guild.message_channel_ids.len())
		.sum::<usize>()
		+ fixtures.private_channel_ids.len();
	if total == 0 {
		return None;
	}
	let mut index = rng.random_range(0..total);
	let mut resolved = None;
	for guild in guilds {
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
	let (content, flags) = if rng.random_bool(0.15) {
		let flags = if rng.random_bool(SILENT_MENTION_CHANCE) {
			MessageFlags::SUPPRESS_NOTIFICATIONS
		} else {
			MessageFlags::empty()
		};
		(format!("<@{CURRENT_USER_ID}> {sentence}"), flags)
	} else {
		(sentence.to_owned(), MessageFlags::empty())
	};
	Some(Target {
		channel_id,
		author,
		content,
		flags,
	})
}

fn thread_target(shared: &Shared) -> Option<Target> {
	let (thread_id, guild_id) = {
		let store = shared.store.lock();
		let thread = store
			.channels
			.values()
			.find(|channel| store.joined_threads.contains(&channel.id.0) && channel.guild_id.is_some())?;
		(thread.id.0, thread.guild_id?.0)
	};
	let author = shared.fixtures.guild(guild_id)?.members.get(1)?.user.clone()?;
	let sentence = *pick(&mut live_rng(), SENTENCES);
	Some(Target {
		channel_id: thread_id,
		author,
		content: format!("<@{CURRENT_USER_ID}> {sentence}"),
		flags: MessageFlags::empty(),
	})
}

struct Posted {
	channel_id: u64,
	id: u64,
	author_id: u64,
}

async fn event_loop(shared: Arc<Shared>) {
	let mut shutdown = shared.shutdown.clone();
	while !interrupted(&mut shutdown, EVENT_START_INTERVAL).await {
		let Some((guild_id, event_id)) = shared.next_scheduled_event() else {
			return;
		};
		let started = json!({ "status": i64::from(ScheduledEventStatus::Active) });
		if shared
			.modify_scheduled_event(guild_id, event_id, &started)
			.is_ok()
		{
			tracing::info!(guild_id, event_id, "live event start");
		}
	}
}

async fn sound_loop(shared: Arc<Shared>) {
	let mut shutdown = shared.shutdown.clone();
	while !interrupted(&mut shutdown, SOUND_INTERVAL).await {
		shared.play_fixture_sound();
	}
}

async fn message_loop(shared: Arc<Shared>) {
	let mut shutdown = shared.shutdown.clone();
	let mut recent: VecDeque<Posted> = VecDeque::new();
	let mut round: u64 = 0;
	loop {
		if interrupted(&mut shutdown, MESSAGE_INTERVAL - TYPING_LEAD).await {
			return;
		}
		let thread_round = round % THREAD_MENTION_EVERY == THREAD_MENTION_EVERY - 1;
		let target = if thread_round {
			thread_target(&shared).or_else(|| pick_target(&shared))
		} else {
			pick_target(&shared)
		};
		let Some(target) = target else {
			continue;
		};
		if shared.typing(target.channel_id, target.author.id.0).is_err() {
			continue;
		}
		if interrupted(&mut shutdown, TYPING_LEAD).await {
			return;
		}
		let draft = Draft {
			flags: target.flags,
			..Draft::text(target.content)
		};
		let posted = shared.post_message(target.channel_id, &target.author, draft);
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
		if round.is_multiple_of(STORM_EVERY) {
			react_in_bulk(&shared, &recent);
		}
		if round.is_multiple_of(POLL_VOTE_EVERY) {
			vote_in_poll(&shared);
		}
	}
}

fn vote_in_poll(shared: &Shared) {
	let Some((channel_id, message_id, answers)) = shared.open_poll() else {
		return;
	};
	let mut rng = live_rng();
	let voters: Vec<u64> = shared
		.fixtures
		.users
		.keys()
		.copied()
		.filter(|&id| id != CURRENT_USER_ID)
		.collect();
	if voters.is_empty() || answers.is_empty() {
		return;
	}
	let user_id = *pick(&mut rng, &voters);
	let answer_id = *pick(&mut rng, &answers);
	if shared
		.cast_poll_vote(user_id, channel_id, message_id, answer_id)
		.is_ok()
	{
		tracing::info!(channel_id, "live poll vote");
	}
}

fn react_in_bulk(shared: &Shared, recent: &VecDeque<Posted>) {
	let Some(posted) = recent.back() else {
		return;
	};
	let mut rng = live_rng();
	let size = rng.random_range(STORM_USERS);
	let user_ids: Vec<u64> = shared
		.fixtures
		.users
		.keys()
		.copied()
		.filter(|&id| id != CURRENT_USER_ID)
		.take(size)
		.collect();
	let emoji = *pick(&mut rng, &STORM_EMOJI);
	if shared
		.reaction_storm(posted.channel_id, posted.id, emoji, &user_ids)
		.is_ok()
	{
		tracing::info!(channel_id = posted.channel_id, "live reaction storm");
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
