use protocol::Snowflake;
use smallvec::SmallVec;

const MAX_TRACKED_IDS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdSet {
	Ids(SmallVec<[Snowflake; 4]>),
	All,
}

impl Default for IdSet {
	fn default() -> Self {
		Self::Ids(SmallVec::new())
	}
}

impl IdSet {
	pub fn insert(&mut self, id: Snowflake) {
		let Self::Ids(ids) = self else { return };
		if ids.contains(&id) {
			return;
		}
		if ids.len() >= MAX_TRACKED_IDS {
			*self = Self::All;
		} else {
			ids.push(id);
		}
	}

	pub fn mark_all(&mut self) {
		*self = Self::All;
	}

	pub fn contains(&self, id: Snowflake) -> bool {
		match self {
			Self::Ids(ids) => ids.contains(&id),
			Self::All => true,
		}
	}

	pub fn is_empty(&self) -> bool {
		matches!(self, Self::Ids(ids) if ids.is_empty())
	}

	pub fn ids(&self) -> Option<&[Snowflake]> {
		match self {
			Self::Ids(ids) => Some(ids),
			Self::All => None,
		}
	}

	pub fn merge(&mut self, other: &IdSet) {
		match other {
			Self::All => self.mark_all(),
			Self::Ids(ids) => {
				for &id in ids {
					self.insert(id);
				}
			}
		}
	}
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
	pub guild_list: bool,
	pub private_channels: bool,
	pub current_user: bool,
	pub own_status: bool,
	pub user_settings: bool,
	pub relationships: bool,
	pub read_state: bool,
	pub alerts: bool,
	pub session: bool,
	pub selection: bool,
	pub search: bool,
	pub plugin_slots: bool,
	pub plugins: bool,
	pub channel_lists: IdSet,
	pub messages: IdSet,
	pub members: IdSet,
	pub presence: IdSet,
	pub typing: IdSet,
	pub voice: IdSet,
	pub voice_session: bool,
	pub calls: bool,
	pub drafts: IdSet,
	pub loaded: IdSet,
	pub streams: bool,
	pub rejected: bool,
}

impl Changes {
	pub fn everything() -> Self {
		Self {
			guild_list: true,
			private_channels: true,
			current_user: true,
			own_status: true,
			user_settings: true,
			relationships: true,
			read_state: true,
			alerts: true,
			session: true,
			selection: true,
			search: true,
			plugin_slots: true,
			plugins: true,
			channel_lists: IdSet::All,
			messages: IdSet::All,
			members: IdSet::All,
			presence: IdSet::All,
			typing: IdSet::All,
			voice: IdSet::All,
			voice_session: true,
			calls: true,
			drafts: IdSet::All,
			loaded: IdSet::All,
			streams: true,
			rejected: true,
		}
	}

	pub fn is_empty(&self) -> bool {
		!(self.guild_list
			|| self.private_channels
			|| self.current_user
			|| self.own_status
			|| self.user_settings
			|| self.relationships
			|| self.read_state
			|| self.alerts
			|| self.session
			|| self.selection
			|| self.search
			|| self.plugin_slots
			|| self.plugins
			|| self.voice_session
			|| self.calls
			|| self.streams
			|| self.rejected)
			&& self.channel_lists.is_empty()
			&& self.messages.is_empty()
			&& self.members.is_empty()
			&& self.presence.is_empty()
			&& self.typing.is_empty()
			&& self.voice.is_empty()
			&& self.drafts.is_empty()
			&& self.loaded.is_empty()
	}

	pub fn merge(&mut self, other: Changes) {
		self.guild_list |= other.guild_list;
		self.private_channels |= other.private_channels;
		self.current_user |= other.current_user;
		self.own_status |= other.own_status;
		self.user_settings |= other.user_settings;
		self.relationships |= other.relationships;
		self.read_state |= other.read_state;
		self.alerts |= other.alerts;
		self.session |= other.session;
		self.selection |= other.selection;
		self.search |= other.search;
		self.plugin_slots |= other.plugin_slots;
		self.plugins |= other.plugins;
		self.voice_session |= other.voice_session;
		self.calls |= other.calls;
		self.streams |= other.streams;
		self.rejected |= other.rejected;
		self.channel_lists.merge(&other.channel_lists);
		self.messages.merge(&other.messages);
		self.members.merge(&other.members);
		self.presence.merge(&other.presence);
		self.typing.merge(&other.typing);
		self.voice.merge(&other.voice);
		self.drafts.merge(&other.drafts);
		self.loaded.merge(&other.loaded);
	}
}
