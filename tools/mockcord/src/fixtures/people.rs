use chrono::Duration;
use protocol::{
	Channel, ChannelType, LenientVec, PremiumType, Relationship, RelationshipKind, Snowflake, User, UserFlags,
};
use rand::Rng;
use rand::rngs::StdRng;

use super::messages::{self, ChannelSpec};
use super::{Builder, CURRENT_USER_ID, corpus};
use crate::ids::EntityIds;

pub struct People {
	pub current: User,
	pub pool: Vec<User>,
}

const FRIEND_COUNT: usize = 8;
const GROUP_NAME: &str = "weekend crew";

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
	let pool = (0..size).map(|index| pool_user(rng, ids, index)).collect();
	People { current, pool }
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
		public_flags: UserFlags::empty(),
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
	let layouts: [(ChannelType, &[usize], Option<&str>); 5] = [
		(ChannelType::Dm, &[0], None),
		(ChannelType::Dm, &[1], None),
		(ChannelType::Dm, &[2], None),
		(ChannelType::GroupDm, &[3, 4, 5], Some(GROUP_NAME)),
		(ChannelType::GroupDm, &[6, 7], None),
	];
	let mut channel_ids = Vec::with_capacity(layouts.len());
	for (variant, (kind, recipients, name)) in layouts.into_iter().enumerate() {
		let channel_id = builder.ids.next();
		let recipients: Vec<User> = recipients
			.iter()
			.map(|&index| builder.people.pool[index].clone())
			.collect();
		let mut authors: Vec<&User> = vec![&builder.people.current];
		authors.extend(recipients.iter());
		let spec = ChannelSpec {
			channel_id,
			guild_id: None,
			count: builder.config.messages_per_channel,
			specials: true,
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
		]);
	kinds
		.map(|(index, kind)| Relationship {
			id: pool[index].id,
			kind,
			user: Some(pool[index].clone()),
			user_id: None,
			nickname: None,
			since: Some(builder.anchor - Duration::days(30 + index as i64 * 17)),
		})
		.collect()
}
