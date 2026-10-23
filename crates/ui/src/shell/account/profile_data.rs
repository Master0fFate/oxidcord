use gpui::{Hsla, SharedString};
use protocol::{RelationshipKind, Snowflake};
use state::State;

use crate::lookup::user_avatar;
use crate::model::{Status, role_color, status_of};

const AVATAR_PIXELS: u32 = 160;

pub struct Badge {
	pub icon_url: String,
	pub description: SharedString,
}

pub struct RoleChip {
	pub name: String,
	pub color: Option<Hsla>,
}

pub struct MutualGuild {
	pub id: Snowflake,
	pub name: SharedString,
	pub icon: Option<String>,
}

pub struct MutualFriend {
	pub name: SharedString,
	pub avatar: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
	None,
	Friend,
	Outgoing,
	Incoming,
	Blocked,
}

pub struct ProfileData {
	pub id: Snowflake,
	pub name: SharedString,
	pub username: String,
	pub handle: SharedString,
	pub avatar: String,
	pub banner_url: Option<String>,
	pub accent: Option<Hsla>,
	pub pronouns: Option<SharedString>,
	pub bio: Option<String>,
	pub badges: Vec<Badge>,
	pub roles: Vec<RoleChip>,
	pub created: chrono::NaiveDate,
	pub joined: Option<chrono::NaiveDate>,
	pub mutual_guilds: Vec<MutualGuild>,
	pub mutual_friends: Vec<MutualFriend>,
	pub mutual_friend_count: usize,
	pub connections: Vec<(String, String)>,
	pub status: Status,
	pub relation: Relation,
	pub mine: bool,
	pub bot: bool,
}

fn roles(state: &State, guild: Option<Snowflake>, user: Snowflake) -> Vec<RoleChip> {
	let Some(guild) = guild.and_then(|id| state.guild(id)) else {
		return Vec::new();
	};
	let Some(member) = guild.members.get(&user) else {
		return Vec::new();
	};
	let mut found: Vec<_> = member
		.roles
		.iter()
		.filter(|id| **id != guild.id)
		.filter_map(|id| guild.roles.get(id))
		.collect();
	found.sort_by_key(|role| std::cmp::Reverse(role.position));
	found
		.into_iter()
		.map(|role| RoleChip {
			name: role.name.clone(),
			color: u32::try_from(role.color)
				.ok()
				.filter(|color| *color != 0)
				.map(role_color),
		})
		.collect()
}

fn relation(state: &State, user: Snowflake) -> Relation {
	state
		.relationships()
		.find(|relationship| relationship.id == user)
		.map_or(Relation::None, |relationship| match relationship.kind {
			RelationshipKind::Friend => Relation::Friend,
			RelationshipKind::OutgoingRequest => Relation::Outgoing,
			RelationshipKind::IncomingRequest => Relation::Incoming,
			RelationshipKind::Blocked => Relation::Blocked,
			_ => Relation::None,
		})
}

impl ProfileData {
	pub fn load(state: &State, user_id: Snowflake, guild: Option<Snowflake>) -> Option<Self> {
		let user = state.user(user_id)?;
		let profile = state.profile(user_id);
		let guild_state = guild.and_then(|id| state.guild(id));
		let member = guild_state.and_then(|guild| guild.members.get(&user_id));
		let member_avatar = member.and_then(|member| member.avatar.as_deref());
		let avatar = match (guild, member_avatar) {
			(Some(guild), Some(hash)) => {
				media::cdn::guild_member_avatar_url(guild.get(), user_id.get(), hash, AVATAR_PIXELS)
			}
			_ => user_avatar(user, AVATAR_PIXELS),
		};
		let banner_hash = profile
			.and_then(|profile| profile.banner.clone())
			.or_else(|| user.banner.clone());
		let accent = profile
			.and_then(|profile| profile.accent_color)
			.or(user.accent_color)
			.and_then(|color| u32::try_from(color).ok())
			.map(role_color);
		let handle = match user.discriminator.as_deref() {
			Some(tag) if tag != "0" && !tag.is_empty() => format!("{}#{tag}", user.username),
			_ => user.username.clone(),
		};
		let joined = member
			.and_then(|member| member.joined_at)
			.map(|time| time.date_naive());
		let mutual_guilds = profile
			.map(|profile| {
				profile
					.mutual_guilds
					.iter()
					.filter_map(|id| state.guild(*id))
					.map(|guild| MutualGuild {
						id: guild.id,
						name: guild.name.clone().into(),
						icon: guild.icon.clone(),
					})
					.collect()
			})
			.unwrap_or_default();
		let mutual_friends: Vec<MutualFriend> = profile
			.map(|profile| {
				profile
					.mutual_friends
					.iter()
					.map(|id| MutualFriend {
						name: state.display_name(None, *id).into(),
						avatar: crate::lookup::avatar_for(state, *id, 64),
					})
					.collect()
			})
			.unwrap_or_default();
		Some(Self {
			id: user_id,
			name: state.display_name(guild, user_id).into(),
			username: user.username.clone(),
			handle: handle.into(),
			avatar,
			banner_url: banner_hash.map(|hash| media::cdn::banner_url(user_id.get(), &hash, 600)),
			accent,
			pronouns: profile
				.and_then(|profile| profile.pronouns.clone())
				.map(Into::into),
			bio: profile.and_then(|profile| profile.bio.clone()),
			badges: profile
				.map(|profile| {
					profile
						.badges
						.iter()
						.map(|badge| Badge {
							icon_url: media::cdn::badge_icon_url(&badge.icon),
							description: badge.description.clone().into(),
						})
						.collect()
				})
				.unwrap_or_default(),
			roles: roles(state, guild, user_id),
			created: user_id.timestamp().date_naive(),
			joined,
			mutual_friend_count: profile.map_or(0, |profile| profile.mutual_friends_count as usize),
			mutual_guilds,
			mutual_friends,
			connections: profile
				.map(|profile| {
					profile
						.connected_accounts
						.iter()
						.map(|account| (account.kind.clone(), account.name.clone()))
						.collect()
				})
				.unwrap_or_default(),
			status: status_of(state, user_id),
			relation: relation(state, user_id),
			mine: state.current_user_id() == Some(user_id),
			bot: user.bot == Some(true),
		})
	}
}
