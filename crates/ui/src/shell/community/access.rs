use protocol::{Permissions, Snowflake};
use state::State;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuildAccess {
	pub permissions: Permissions,
	pub owner: bool,
	pub top_role: i64,
}

impl GuildAccess {
	pub fn of(state: &State, guild_id: Snowflake) -> Self {
		let Some(me) = state.current_user_id() else {
			return Self::default();
		};
		let Some(guild) = state.guild(guild_id) else {
			return Self::default();
		};
		let top_role = guild
			.members
			.get(&me)
			.map_or(0, |member| top_position(state, guild_id, &member.roles));
		Self {
			permissions: state.guild_permissions(guild_id, me),
			owner: guild.owner_id == Some(me),
			top_role,
		}
	}

	pub fn has(&self, permission: Permissions) -> bool {
		self.permissions.contains(Permissions::ADMINISTRATOR) || self.permissions.contains(permission)
	}

	pub fn has_any(&self, permissions: Permissions) -> bool {
		self.permissions.contains(Permissions::ADMINISTRATOR) || self.permissions.intersects(permissions)
	}

	pub fn can_open_settings(&self) -> bool {
		self.has_any(SETTINGS_PERMISSIONS)
	}

	pub fn outranks(&self, state: &State, guild_id: Snowflake, target: Snowflake) -> bool {
		let Some(guild) = state.guild(guild_id) else {
			return false;
		};
		if guild.owner_id == Some(target) {
			return false;
		}
		if self.owner {
			return true;
		}
		let target_top = guild
			.members
			.get(&target)
			.map_or(0, |member| top_position(state, guild_id, &member.roles));
		self.top_role > target_top
	}
}

pub const MODERATION_PERMISSIONS: Permissions = Permissions::MANAGE_NICKNAMES
	.union(Permissions::MANAGE_ROLES)
	.union(Permissions::MOVE_MEMBERS)
	.union(Permissions::MUTE_MEMBERS)
	.union(Permissions::DEAFEN_MEMBERS)
	.union(Permissions::MODERATE_MEMBERS)
	.union(Permissions::KICK_MEMBERS)
	.union(Permissions::BAN_MEMBERS);

pub const SETTINGS_PERMISSIONS: Permissions = Permissions::MANAGE_GUILD
	.union(Permissions::MANAGE_ROLES)
	.union(Permissions::MANAGE_GUILD_EXPRESSIONS)
	.union(Permissions::VIEW_AUDIT_LOG)
	.union(Permissions::BAN_MEMBERS)
	.union(Permissions::KICK_MEMBERS)
	.union(Permissions::MODERATE_MEMBERS);

pub fn top_position(state: &State, guild_id: Snowflake, roles: &[Snowflake]) -> i64 {
	state.guild(guild_id).map_or(0, |guild| {
		roles
			.iter()
			.filter_map(|id| guild.roles.get(id))
			.map(|role| role.position)
			.max()
			.unwrap_or(0)
	})
}
