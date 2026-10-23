use protocol::{
	Channel, Guild, Member, Message, Presence, Ready, ReadySupplemental, Snowflake, StatusKind, VoiceState,
};

use crate::changes::Changes;
use crate::guild::{GuildState, stripped_member};
use crate::state::{State, intern_user};

impl State {
	pub(crate) fn apply_ready(&mut self, ready: &Ready) -> Changes {
		self.reset_for_ready();
		self.current_user = Some(ready.user.clone());
		intern_user(&mut self.users, &ready.user);
		self.session.session_id.clone_from(&ready.session_id);
		self.session.resume_url.clone_from(&ready.resume_gateway_url);
		self.session.connected = true;
		for user in ready.users.iter() {
			intern_user(&mut self.users, user);
		}
		warn_dropped("guild", ready.guilds.dropped());
		warn_dropped("private_channel", ready.private_channels.dropped());
		for (index, guild) in ready.guilds.iter().enumerate() {
			self.ingest_guild(guild);
			if let Some(members) = ready.merged_members.get(index) {
				self.ingest_members(guild.id, members);
			}
		}
		for channel in ready.private_channels.iter() {
			self.store_private_channel(channel);
		}
		for relationship in ready.relationships.iter() {
			self.store_relationship(relationship);
		}
		for read_state in ready.read_state.entries.iter() {
			if read_state
				.read_state_type
				.is_none_or(|kind| kind == protocol::ReadStateKind::Channel)
			{
				self.read_states.insert(read_state.id, read_state.clone());
			}
		}
		for settings in ready.user_guild_settings.entries.iter() {
			self.store_guild_settings(settings);
		}
		if let Some(settings) = &ready.user_settings {
			self.apply_user_settings(settings);
		}
		let legacy_folders = ready
			.user_settings
			.as_ref()
			.and_then(|settings| settings.get("guild_folders"))
			.and_then(serde_json::Value::as_array)
			.is_some_and(|folders| !folders.is_empty());
		if !legacy_folders && let Some(proto) = &ready.user_settings_proto {
			self.apply_settings_proto(proto);
		}
		self.reorder_guilds();
		Changes::everything()
	}

	pub(crate) fn apply_supplemental(&mut self, supplemental: &ReadySupplemental) -> Changes {
		let mut changes = Changes::default();
		for (index, guild) in supplemental.guilds.iter().enumerate() {
			let guild_id = guild.id;
			if let Some(state) = self.guilds.get_mut(&guild_id) {
				state.voice_states = guild
					.voice_states
					.iter()
					.filter(|voice| voice.channel_id.is_some())
					.map(|voice| {
						(
							voice.user_id,
							VoiceState {
								guild_id: Some(guild_id),
								..voice.clone()
							},
						)
					})
					.collect();
				changes.voice.insert(guild_id);
			}
			if let Some(members) = supplemental.merged_members.get(index) {
				self.ingest_members(guild_id, members);
				changes.members.insert(guild_id);
			}
			if let Some(presences) = supplemental.merged_presences.guilds.get(index) {
				for presence in presences.iter() {
					if let Some(user_id) = self.apply_presence(presence) {
						changes.presence.insert(user_id);
					}
				}
			}
		}
		for presence in supplemental.merged_presences.friends.iter() {
			if let Some(user_id) = self.apply_presence(presence) {
				changes.presence.insert(user_id);
			}
		}
		changes.guild_list = true;
		changes
	}

	fn reset_for_ready(&mut self) {
		self.guilds.clear();
		self.private_channels.clear();
		self.channel_guilds.clear();
		self.users.clear();
		self.presences.clear();
		self.relationships.clear();
		self.read_states.clear();
		self.guild_settings.clear();
		self.global_settings = None;
		self.typing.clear();
	}

	pub(crate) fn ingest_guild(&mut self, guild: &Guild) {
		let id = guild.id;
		let mut state = GuildState::from_guild(guild);
		for channel in guild.channels.iter().chain(guild.threads.iter()) {
			let mut stored = channel.clone();
			stored.guild_id = Some(id);
			self.channel_guilds.insert(stored.id, Some(id));
			state.insert_channel(stored);
		}
		for member in guild.members.iter() {
			if let Some(user) = &member.user {
				intern_user(&mut self.users, user);
				state.members.insert(user.id, stripped_member(member));
			}
		}
		for voice in guild
			.voice_states
			.iter()
			.filter(|voice| voice.channel_id.is_some())
		{
			state.voice_states.insert(
				voice.user_id,
				VoiceState {
					guild_id: Some(id),
					..voice.clone()
				},
			);
		}
		self.guilds.insert(id, state);
		for presence in guild.presences.iter() {
			self.apply_presence(presence);
		}
	}

	pub(crate) fn ingest_members(&mut self, guild_id: Snowflake, members: &[Member]) {
		let Some(guild) = self.guilds.get_mut(&guild_id) else {
			return;
		};
		for member in members {
			if let Some(user) = &member.user {
				intern_user(&mut self.users, user);
				guild.members.insert(user.id, stripped_member(member));
			}
		}
	}

	pub(crate) fn ingest_message_people(&mut self, guild_id: Option<Snowflake>, message: &Message) {
		intern_user(&mut self.users, &message.author);
		for user in message.mentions.iter() {
			intern_user(&mut self.users, user);
		}
		let Some(member) = &message.member else {
			return;
		};
		let Some(guild) = guild_id.and_then(|id| self.guilds.get_mut(&id)) else {
			return;
		};
		guild.members.insert(message.author.id, stripped_member(member));
	}

	pub(crate) fn store_private_channel(&mut self, channel: &Channel) {
		let mut stored = channel.clone();
		let recipients = std::mem::take(&mut stored.recipients);
		for user in recipients.iter() {
			intern_user(&mut self.users, user);
		}
		if stored.recipient_ids.is_empty() {
			stored.recipient_ids = recipients.iter().map(|user| user.id).collect();
		}
		stored.guild_id = None;
		self.channel_guilds.insert(stored.id, None);
		self.private_channels.insert(stored.id, stored);
	}

	pub(crate) fn store_relationship(&mut self, relationship: &protocol::Relationship) {
		if let Some(user) = &relationship.user {
			intern_user(&mut self.users, user);
		}
		self.relationships.insert(
			relationship.id,
			protocol::Relationship {
				id: relationship.id,
				kind: relationship.kind,
				user: None,
				user_id: Some(relationship.id),
				nickname: relationship.nickname.clone(),
				since: relationship.since,
			},
		);
	}

	pub(crate) fn apply_presence(&mut self, presence: &Presence) -> Option<Snowflake> {
		let user_id = presence
			.user_id
			.or_else(|| presence.user.as_ref().map(|user| user.id))?;
		self.set_presence(user_id, presence);
		Some(user_id)
	}

	pub(crate) fn set_presence(&mut self, user_id: Snowflake, presence: &Presence) {
		if let Some(user) = &presence.user {
			intern_user(&mut self.users, user);
		}
		if matches!(presence.status, StatusKind::Offline | StatusKind::Invisible) {
			self.presences.remove(&user_id);
			return;
		}
		self.presences.insert(
			user_id,
			Presence {
				user: None,
				user_id: Some(user_id),
				guild_id: presence.guild_id,
				status: presence.status.clone(),
				activities: presence.activities.clone(),
				client_status: presence.client_status.clone(),
			},
		);
	}
}

fn warn_dropped(kind: &str, dropped: usize) {
	if dropped > 0 {
		tracing::warn!(kind, dropped, "dropped malformed elements from READY");
	}
}
