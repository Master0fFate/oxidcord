use chrono::Duration;
use protocol::{
	Channel, ChannelType, LenientVec, PremiumType, Relationship, RelationshipKind, Snowflake, User, UserFlags,
};
use rand::Rng;
use rand::rngs::StdRng;

use super::messages::{self, ChannelSpec};
use super::{Builder, CURRENT_USER_ID, SECOND_USER_ID, corpus};
use crate::ids::EntityIds;

pub struct People {
	pub current: User,
	pub second: User,
	pub pool: Vec<User>,
}

const FRIEND_COUNT: usize = 8;
const NICKNAMED_FRIEND: usize = 1;
const IGNORED_FRIEND: usize = 2;
const NICKNAME: &str = "Bestie";
const GROUP_NAME: &str = "weekend crew";
const REQUEST_MESSAGES: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Standing {
	Accepted,
	Request,
	Spam,
}

pub fn generate(rng: &mut StdRng, ids: &mut EntityIds, size: usize) -> People {
	let current = User {
		id: Snowflake(CURRENT_USER_ID),
		username: "oxid".to_owned(),
		discriminator: Some("0".to_owned()),
		global_name: Some("Oxid".to_owned()),
		avatar: Some(corpus::hex_hash(rng)),
		bot: Some(false),
		premium_type: Some(PremiumType::Nitro),
		email: Some("oxid@mock.invalid".to_owned()),
		verified: Some(true),
		mfa_enabled: Some(false),
		locale: Some("en-US".to_owned()),
		..User::default()
	};
	let second = User {
		id: Snowflake(SECOND_USER_ID),
		username: "alto".to_owned(),
		global_name: Some("Alto".to_owned()),
		avatar: Some(corpus::hex_hash(rng)),
		email: Some("alto@mock.invalid".to_owned()),
		..current.clone()
	};
	let pool = (0..size).map(|index| pool_user(rng, ids, index)).collect();
	People {
		current,
		second,
		pool,
	}
}

pub fn grow_pool(builder: &mut Builder, size: usize) {
	for index in builder.people.pool.len()..size {
		let user = pool_user(&mut builder.rng, &mut builder.ids, index);
		builder.people.pool.push(user);
	}
}

fn pool_user(rng: &mut StdRng, ids: &mut EntityIds, index: usize) -> User {
	let adjective = *corpus::pick(rng, corpus::ADJECTIVES);
	let noun = *corpus::pick(rng, corpus::NOUNS);
	let is_bot = index % 17 == 16;
	let username = if is_bot {
		format!("{adjective}{noun}bot")
	} else {
		format!("{adjective}{noun}{index}")
	};
	let global_name = format!("{} {}", capitalized(adjective), capitalized(noun));
	User {
		id: Snowflake(ids.next()),
		username,
		discriminator: Some("0".to_owned()),
		global_name: Some(global_name),
		avatar: rng.random_bool(0.7).then(|| corpus::hex_hash(rng)),
		bot: Some(is_bot),
		public_flags: if index == 16 {
			UserFlags::VERIFIED_BOT
		} else {
			UserFlags::empty()
		},
		..User::default()
	}
}

fn capitalized(word: &str) -> String {
	let mut chars = word.chars();
	chars
		.next()
		.map(|first| first.to_uppercase().chain(chars).collect())
		.unwrap_or_default()
}

pub fn build_private_channels(builder: &mut Builder) -> Vec<u64> {
	let layouts: [(ChannelType, &[usize], Option<&str>, Standing); 7] = [
		(ChannelType::Dm, &[0], None, Standing::Accepted),
		(ChannelType::Dm, &[1], None, Standing::Accepted),
		(ChannelType::Dm, &[2], None, Standing::Accepted),
		(
			ChannelType::GroupDm,
			&[3, 4, 5],
			Some(GROUP_NAME),
			Standing::Accepted,
		),
		(ChannelType::GroupDm, &[6, 7], None, Standing::Accepted),
		(ChannelType::Dm, &[20], None, Standing::Request),
		(ChannelType::Dm, &[21], None, Standing::Spam),
	];
	let mut channel_ids = Vec::with_capacity(layouts.len());
	for (variant, (kind, recipients, name, standing)) in layouts.into_iter().enumerate() {
		let channel_id = builder.ids.next();
		let recipients: Vec<User> = recipients
			.iter()
			.map(|&index| builder.people.pool[index].clone())
			.collect();
		let mut authors: Vec<&User> = Vec::new();
		if standing == Standing::Accepted {
			authors.push(&builder.people.current);
		}
		authors.extend(recipients.iter());
		let spec = ChannelSpec {
			channel_id,
			guild_id: None,
			count: match standing {
				Standing::Accepted => builder.config.messages_per_channel,
				_ => REQUEST_MESSAGES.min(builder.config.messages_per_channel),
			},
			specials: standing == Standing::Accepted,
			variant,
			anchor: builder.anchor,
			me: &builder.people.current,
			authors: &authors,
			roles: &[],
			emojis: &[],
			channels: &[],
			threads: &[],
		};
		let messages = messages::generate(&mut builder.rng, &spec);
		let owner_id = (kind == ChannelType::GroupDm).then_some(Snowflake(CURRENT_USER_ID));
		let channel = Channel {
			id: Snowflake(channel_id),
			kind,
			name: name.map(str::to_owned),
			icon: (kind == ChannelType::GroupDm && name.is_some())
				.then(|| corpus::hex_hash(&mut builder.rng)),
			owner_id,
			last_message_id: messages.keys().next_back().copied().map(Snowflake),
			recipient_ids: recipients.iter().map(|user| user.id).collect(),
			recipients: LenientVec::from(recipients),
			is_message_request: (standing != Standing::Accepted).then_some(true),
			is_message_request_timestamp: (standing != Standing::Accepted)
				.then(|| builder.anchor - Duration::hours(variant as i64)),
			is_spam: (standing == Standing::Spam).then_some(true),
			..Channel::default()
		};
		builder.channels.insert(channel_id, channel);
		builder.messages.insert(channel_id, messages);
		channel_ids.push(channel_id);
	}
	channel_ids
}

pub fn build_relationships(builder: &Builder) -> Vec<Relationship> {
	let pool = &builder.people.pool;
	let kinds = (0..FRIEND_COUNT)
		.map(|index| (index, RelationshipKind::Friend))
		.chain([
			(FRIEND_COUNT, RelationshipKind::IncomingRequest),
			(FRIEND_COUNT + 1, RelationshipKind::OutgoingRequest),
			(FRIEND_COUNT + 2, RelationshipKind::Blocked),
			(FRIEND_COUNT + 3, RelationshipKind::IncomingRequest),
		]);
	kinds
		.map(|(index, kind)| Relationship {
			id: pool[index].id,
			kind,
			user: Some(pool[index].clone()),
			user_id: None,
			nickname: (index == NICKNAMED_FRIEND).then(|| NICKNAME.to_owned()),
			since: Some(builder.anchor - Duration::days(30 + index as i64 * 17)),
			user_ignored: index == IGNORED_FRIEND,
			is_spam_request: index == FRIEND_COUNT + 3,
			stranger_request: index == FRIEND_COUNT,
			should_notify: false,
		})
		.collect()
}
