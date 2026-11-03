use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::channel::Channel;
use crate::emoji::{Emoji, Sticker};
use crate::lenient::{LenientVec, int_enum, timestamp};
use crate::member::Member;
use crate::presence::Presence;
use crate::role::Role;
use crate::snowflake::Snowflake;
use crate::voice::VoiceState;

int_enum! {
	pub enum VerificationLevel {
		None = 0,
		Low = 1,
		Medium = 2,
		High = 3,
		VeryHigh = 4,
	}
}

int_enum! {
	pub enum DefaultMessageNotifications {
		AllMessages = 0,
		OnlyMentions = 1,
	}
}

int_enum! {
	pub enum ExplicitContentFilter {
		Disabled = 0,
		MembersWithoutRoles = 1,
		AllMembers = 2,
	}
}

int_enum! {
	pub enum MfaLevel {
		None = 0,
		Elevated = 1,
	}
}

int_enum! {
	pub enum PremiumTier {
		None = 0,
		Tier1 = 1,
		Tier2 = 2,
		Tier3 = 3,
	}
}

int_enum! {
	pub enum NsfwLevel {
		Default = 0,
		Explicit = 1,
		Safe = 2,
		AgeRestricted = 3,
	}
}

int_enum! {
	pub enum ScheduledEventStatus {
		Scheduled = 1,
		Active = 2,
		Completed = 3,
		Canceled = 4,
	}
}

int_enum! {
	pub enum ScheduledEventKind {
		StageInstance = 1,
		Voice = 2,
		External = 3,
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledEventMetadata {
	pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledEvent {
	pub id: Snowflake,
	pub guild_id: Snowflake,
	pub channel_id: Option<Snowflake>,
	pub creator_id: Option<Snowflake>,
	pub name: String,
	pub description: Option<String>,
	#[serde(deserialize_with = "timestamp")]
	pub scheduled_start_time: Option<DateTime<Utc>>,
	#[serde(deserialize_with = "timestamp")]
	pub scheduled_end_time: Option<DateTime<Utc>>,
	pub status: ScheduledEventStatus,
	pub entity_type: ScheduledEventKind,
	pub entity_metadata: Option<ScheduledEventMetadata>,
	pub user_count: Option<i64>,
	pub image: Option<String>,
	pub recurrence_rule: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledEventRsvp {
	pub guild_scheduled_event_id: Snowflake,
	pub response: i64,
}

impl ScheduledEvent {
	pub fn is_open(&self) -> bool {
		matches!(
			self.status,
			ScheduledEventStatus::Scheduled | ScheduledEventStatus::Active
		)
	}

	pub fn location(&self) -> Option<&str> {
		self.entity_metadata
			.as_ref()
			.and_then(|metadata| metadata.location.as_deref())
			.filter(|location| !location.is_empty())
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UnavailableGuild {
	pub id: Snowflake,
	pub unavailable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuildProperties {
	pub id: Option<Snowflake>,
	pub name: Option<String>,
	pub icon: Option<String>,
	pub splash: Option<String>,
	pub banner: Option<String>,
	pub owner_id: Option<Snowflake>,
	pub afk_channel_id: Option<Snowflake>,
	pub afk_timeout: Option<i64>,
	pub verification_level: Option<VerificationLevel>,
	pub default_message_notifications: Option<DefaultMessageNotifications>,
	pub explicit_content_filter: Option<ExplicitContentFilter>,
	pub features: Vec<String>,
	pub mfa_level: Option<MfaLevel>,
	pub system_channel_id: Option<Snowflake>,
	pub rules_channel_id: Option<Snowflake>,
	pub max_members: Option<i64>,
	pub vanity_url_code: Option<String>,
	pub description: Option<String>,
	pub premium_tier: Option<PremiumTier>,
	pub premium_subscription_count: Option<i64>,
	pub preferred_locale: Option<String>,
	pub nsfw_level: Option<NsfwLevel>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Guild {
	pub id: Snowflake,
	pub name: Option<String>,
	pub icon: Option<String>,
	pub splash: Option<String>,
	pub banner: Option<String>,
	pub owner_id: Option<Snowflake>,
	pub afk_channel_id: Option<Snowflake>,
	pub afk_timeout: Option<i64>,
	pub verification_level: Option<VerificationLevel>,
	pub default_message_notifications: Option<DefaultMessageNotifications>,
	pub explicit_content_filter: Option<ExplicitContentFilter>,
	pub roles: LenientVec<Role>,
	pub emojis: LenientVec<Emoji>,
	pub stickers: LenientVec<Sticker>,
	pub features: Vec<String>,
	pub mfa_level: Option<MfaLevel>,
	pub system_channel_id: Option<Snowflake>,
	pub rules_channel_id: Option<Snowflake>,
	pub max_members: Option<i64>,
	pub vanity_url_code: Option<String>,
	pub description: Option<String>,
	pub premium_tier: Option<PremiumTier>,
	pub premium_subscription_count: Option<i64>,
	pub preferred_locale: Option<String>,
	pub nsfw_level: Option<NsfwLevel>,
	pub channels: LenientVec<Channel>,
	pub threads: LenientVec<Channel>,
	pub members: LenientVec<Member>,
	pub voice_states: LenientVec<VoiceState>,
	pub presences: LenientVec<Presence>,
	pub guild_scheduled_events: LenientVec<ScheduledEvent>,
	pub member_count: Option<i64>,
	#[serde(deserialize_with = "timestamp")]
	pub joined_at: Option<DateTime<Utc>>,
	pub large: Option<bool>,
	pub unavailable: Option<bool>,
	pub properties: Option<Box<GuildProperties>>,
}

impl Guild {
	pub fn normalized(mut self) -> Guild {
		let Some(properties) = self.properties.take() else {
			return self;
		};
		let properties = *properties;
		if self.id == Snowflake::default() {
			self.id = properties.id.unwrap_or_default();
		}
		if self.features.is_empty() {
			self.features = properties.features;
		}
		macro_rules! lift {
			($($field:ident),+ $(,)?) => {
				$(self.$field = self.$field.take().or(properties.$field);)+
			};
		}
		lift!(
			name,
			icon,
			splash,
			banner,
			owner_id,
			afk_channel_id,
			afk_timeout,
			verification_level,
			default_message_notifications,
			explicit_content_filter,
			mfa_level,
			system_channel_id,
			rules_channel_id,
			max_members,
			vanity_url_code,
			description,
			premium_tier,
			premium_subscription_count,
			preferred_locale,
			nsfw_level,
		);
		self
	}
}
