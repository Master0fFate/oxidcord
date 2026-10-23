use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
