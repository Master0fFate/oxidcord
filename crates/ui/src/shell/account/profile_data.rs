use gpui::{Hsla, SharedString};
use protocol::{RelationshipKind, Snowflake};
use state::{State, UserOverride};

use crate::cosmetics::{Flair, flair};
use crate::lookup::user_avatar;
use crate::model::{Status, role_color, status_of};

const AVATAR_PIXELS: u32 = 160;

pub struct Badge {
	pub icon_url: String,
	pub description: SharedString,
}

pub struct RoleChip {
	pub id: Snowflake,
	pub name: String,
	pub color: Option<Hsla>,
	pub own: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Relation {
	#[default]
	None,
	Friend,
	Outgoing,
	Incoming,
	Blocked,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelationFacts {
	pub relation: Relation,
	pub ignored: bool,
	pub nickname: Option<String>,
	pub since: Option<chrono::NaiveDate>,
}

pub struct ProfileData {
	pub id: Snowflake,
	pub name: SharedString,
	pub handle: SharedString,
	pub avatar: String,
	pub banner_url: Option<String>,
	pub accent: Option<Hsla>,
	pub theme_colors: Option<[u32; 2]>,
	pub pronouns: Option<SharedString>,
	pub legacy_username: Option<SharedString>,
	pub private: bool,
	pub flair: Flair,
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
	pub facts: RelationFacts,
	pub mine: bool,
	pub bot: bool,
	pub over: Option<UserOverride>,
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
	let mine = state
		.current_user_id()
		.and_then(|me| guild.members.get(&me))
		.map(|member| member.roles.as_slice())
		.unwrap_or_default();
	found
		.into_iter()
		.map(|role| RoleChip {
			id: role.id,
			own: mine.contains(&role.id),
			name: role.name.clone(),
			color: u32::try_from(role.color)
				.ok()
				.filter(|color| *color != 0)
				.map(role_color),
		})
		.collect()
}

pub fn relation_facts(state: &State, user: Snowflake) -> RelationFacts {
	let Some(relationship) = state.relationship(user) else {
		return RelationFacts::default();
	};
	let relation = match relationship.kind {
		RelationshipKind::Friend => Relation::Friend,
		RelationshipKind::OutgoingRequest => Relation::Outgoing,
		RelationshipKind::IncomingRequest => Relation::Incoming,
		RelationshipKind::Blocked => Relation::Blocked,
		_ => Relation::None,
	};
	RelationFacts {
		relation,
		ignored: relationship.user_ignored,
		nickname: state.friend_nickname(user).map(str::to_owned),
		since: relationship
			.since
			.filter(|_| relation == Relation::Friend)
			.map(|since| since.date_naive()),
	}
}

impl ProfileData {
	pub fn handle_line(&self) -> String {
		let mut line = self.handle.to_string();
		for extra in self.pronouns.iter().chain(&self.legacy_username) {
			line.push_str(" \u{b7} ");
			line.push_str(extra);
		}
		line
	}

	pub fn load(state: &State, user_id: Snowflake, guild: Option<Snowflake>) -> Option<Self> {
		let user = state.user(user_id)?;
		let over = state.user_override(guild, user_id).cloned();
		let profile = state.profile(user_id);
		let guild_state = guild.and_then(|id| state.guild(id));
		let member = guild_state.and_then(|guild| guild.members.get(&user_id));
		let guild_profile = profile
			.and_then(|profile| profile.guild_profile.as_ref())
			.filter(|found| Some(found.guild_id) == guild);
		let loaded_member = profile
			.and_then(|profile| profile.guild_member.as_ref())
			.filter(|_| guild.is_some());
		let member_avatar = member
			.and_then(|member| member.avatar.as_deref())
			.or_else(|| loaded_member.and_then(|member| member.avatar.as_deref()));
		let member_banner = guild_profile
			.and_then(|found| found.banner.as_deref())
			.or_else(|| member.and_then(|member| member.banner.as_deref()))
			.or_else(|| loaded_member.and_then(|member| member.banner.as_deref()));
		let override_avatar = over.as_ref().and_then(|over| over.avatar.clone());
		let avatar = match (override_avatar, guild, member_avatar) {
			(Some(url), _, _) => url,
			(None, Some(guild), Some(hash)) => {
				media::cdn::guild_member_avatar_url(guild.get(), user_id.get(), hash, AVATAR_PIXELS)
			}
			_ => user_avatar(user, AVATAR_PIXELS),
		};
		let banner_hash = profile
			.and_then(|profile| profile.banner.clone())
			.or_else(|| user.banner.clone());
		let accent = guild_profile
			.and_then(|found| found.accent_color)
			.or_else(|| profile.and_then(|profile| profile.accent_color))
			.or(user.accent_color)
			.and_then(|color| u32::try_from(color).ok())
			.map(role_color);
		let theme_colors = guild_profile
			.and_then(|found| found.theme_colors)
			.or_else(|| profile.and_then(|profile| profile.theme_colors));
		let handle = match user.discriminator.as_deref() {
			Some(tag) if tag != "0" && !tag.is_empty() => format!("{}#{tag}", user.username),
			_ => user.username.clone(),
		};
		let joined = member
			.or(loaded_member)
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
			name: over
				.as_ref()
				.and_then(|over| over.name.clone())
				.unwrap_or_else(|| state.display_name(guild, user_id))
				.into(),
			handle: handle.into(),
			avatar,
			banner_url: over.as_ref().and_then(|over| over.banner.clone()).or_else(|| {
				match (guild, member_banner) {
					(Some(guild), Some(hash)) => Some(media::cdn::guild_member_banner_url(
						guild.get(),
						user_id.get(),
						hash,
						600,
					)),
					_ => banner_hash.map(|hash| media::cdn::banner_url(user_id.get(), &hash, 600)),
				}
			}),
			accent,
			theme_colors,
			pronouns: over
				.as_ref()
				.and_then(|over| over.pronouns.clone())
				.or_else(|| guild_profile.and_then(|found| found.pronouns.clone()))
				.or_else(|| profile.and_then(|profile| profile.pronouns.clone()))
				.map(Into::into),
			bio: guild_profile
				.and_then(|found| found.bio.clone())
				.or_else(|| profile.and_then(|profile| profile.bio.clone())),
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
			legacy_username: profile
				.and_then(|profile| profile.legacy_username.clone())
				.map(Into::into),
			private: profile.is_some_and(|profile| profile.private),
			flair: flair(state, guild, user_id),
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
			facts: relation_facts(state, user_id),
			mine: state.current_user_id() == Some(user_id),
			bot: user.bot == Some(true),
			over,
		})
	}
}
