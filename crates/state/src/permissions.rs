use chrono::{DateTime, Utc};
use protocol::{Channel, PermissionOverwriteKind, Permissions, Snowflake};

use crate::guild::{GuildState, is_thread};
use crate::state::State;

pub const DM_PERMISSIONS: Permissions = Permissions::VIEW_CHANNEL
	.union(Permissions::SEND_MESSAGES)
	.union(Permissions::ADD_REACTIONS)
	.union(Permissions::EMBED_LINKS)
	.union(Permissions::ATTACH_FILES)
	.union(Permissions::READ_MESSAGE_HISTORY)
	.union(Permissions::USE_EXTERNAL_EMOJIS)
	.union(Permissions::USE_EXTERNAL_STICKERS)
	.union(Permissions::SEND_VOICE_MESSAGES)
	.union(Permissions::SEND_POLLS);

const TIMEOUT_PERMISSIONS: Permissions = Permissions::VIEW_CHANNEL.union(Permissions::READ_MESSAGE_HISTORY);

pub(crate) fn compute_permissions(
	guild: &GuildState,
	channel: Option<&Channel>,
	user_id: Snowflake,
	now: DateTime<Utc>,
) -> Permissions {
	if guild.owner_id == Some(user_id) {
		return Permissions::all();
	}
	let member = guild.members.get(&user_id);
	let member_roles = member.map_or(&[][..], |member| member.roles.as_slice());
	let mut permissions = guild
		.roles
		.get(&guild.id)
		.map_or(Permissions::empty(), |role| role.permissions);
	for role_id in member_roles {
		if let Some(role) = guild.roles.get(role_id) {
			permissions |= role.permissions;
		}
	}
	if permissions.contains(Permissions::ADMINISTRATOR) {
		return Permissions::all();
	}
	if let Some(channel) = overwrite_source(guild, channel) {
		permissions = apply_overwrites(guild, channel, member_roles, user_id, permissions);
	}
	let timed_out = member
		.and_then(|member| member.communication_disabled_until)
		.is_some_and(|until| until > now);
	if timed_out {
		permissions &= TIMEOUT_PERMISSIONS;
	}
	permissions
}

fn overwrite_source<'a>(guild: &'a GuildState, channel: Option<&'a Channel>) -> Option<&'a Channel> {
	let channel = channel?;
	if is_thread(channel.kind) {
		return channel
			.parent_id
			.and_then(|parent_id| guild.channels.get(&parent_id));
	}
	Some(channel)
}

fn apply_overwrites(
	guild: &GuildState,
	channel: &Channel,
	member_roles: &[Snowflake],
	user_id: Snowflake,
	mut permissions: Permissions,
) -> Permissions {
	let mut role_allow = Permissions::empty();
	let mut role_deny = Permissions::empty();
	let mut member_overwrite = None;
	for overwrite in channel.permission_overwrites.iter() {
		match overwrite.kind {
			PermissionOverwriteKind::Role if overwrite.id == guild.id => {
				permissions &= !overwrite.deny;
				permissions |= overwrite.allow;
			}
			PermissionOverwriteKind::Role if member_roles.contains(&overwrite.id) => {
				role_allow |= overwrite.allow;
				role_deny |= overwrite.deny;
			}
			PermissionOverwriteKind::Member if overwrite.id == user_id => member_overwrite = Some(overwrite),
			_ => {}
		}
	}
	permissions &= !role_deny;
	permissions |= role_allow;
	if let Some(overwrite) = member_overwrite {
		permissions &= !overwrite.deny;
		permissions |= overwrite.allow;
	}
	permissions
}

impl State {
	pub fn permissions_in(
		&self,
		guild_id: Snowflake,
		channel_id: Snowflake,
		user_id: Snowflake,
	) -> Permissions {
		match self.guilds.get(&guild_id) {
			Some(guild) => compute_permissions(guild, guild.channel(channel_id), user_id, Utc::now()),
			None => Permissions::empty(),
		}
	}

	pub fn guild_permissions(&self, guild_id: Snowflake, user_id: Snowflake) -> Permissions {
		match self.guilds.get(&guild_id) {
			Some(guild) => compute_permissions(guild, None, user_id, Utc::now()),
			None => Permissions::empty(),
		}
	}

	pub fn my_permissions(&self, channel_id: Snowflake) -> Permissions {
		let Some(me) = self.current_user_id() else {
			return Permissions::empty();
		};
		match self.channel_guilds.get(&channel_id) {
			Some(Some(guild_id)) => self.permissions_in(*guild_id, channel_id, me),
			Some(None) => DM_PERMISSIONS,
			None => Permissions::empty(),
		}
	}

	pub fn can_view(&self, channel_id: Snowflake) -> bool {
		self.my_permissions(channel_id)
			.contains(Permissions::VIEW_CHANNEL)
	}

	pub fn can_send(&self, channel_id: Snowflake) -> bool {
		let needed = match self.channel(channel_id) {
			Some(channel) if is_thread(channel.kind) => Permissions::SEND_MESSAGES_IN_THREADS,
			_ => Permissions::SEND_MESSAGES,
		};
		self.my_permissions(channel_id)
			.contains(Permissions::VIEW_CHANNEL | needed)
	}

	pub fn can_attach(&self, channel_id: Snowflake) -> bool {
		self.can_send(channel_id)
			&& self
				.my_permissions(channel_id)
				.contains(Permissions::ATTACH_FILES)
	}

	pub fn can_react(&self, channel_id: Snowflake) -> bool {
		self.my_permissions(channel_id).contains(
			Permissions::VIEW_CHANNEL | Permissions::READ_MESSAGE_HISTORY | Permissions::ADD_REACTIONS,
		)
	}

	pub fn can_manage_messages(&self, channel_id: Snowflake) -> bool {
		self.my_permissions(channel_id)
			.contains(Permissions::VIEW_CHANNEL | Permissions::MANAGE_MESSAGES)
	}
}
