use protocol::gateway::capability::{
	AUTH_TOKEN_REFRESH, AUTO_CALL_CONNECT, CLIENT_STATE_V2, DEBOUNCE_MESSAGE_REACTIONS, DEDUPE_USER_OBJECTS,
	LAZY_USER_NOTES, MULTIPLE_GUILD_EXPERIMENT_POPULATIONS, NON_CHANNEL_READ_STATES, PASSIVE_GUILD_UPDATE,
	PASSIVE_GUILD_UPDATE_V2, PRIORITIZED_READY_PAYLOAD, USER_SETTINGS_PROTO, VERSIONED_READ_STATES,
	VERSIONED_USER_GUILD_SETTINGS,
};
use protocol::{IdentifyProperties, Token};

#[derive(Debug)]
pub struct GatewayConfig {
	pub token: Token,
	pub gateway_url: String,
	pub identify: IdentifyProperties,
	pub capabilities: u64,
	pub large_threshold: u32,
}

impl GatewayConfig {
	pub const DEFAULT_GATEWAY_URL: &'static str = "wss://gateway.discord.gg";
	pub const DEFAULT_CAPABILITIES: u64 = LAZY_USER_NOTES
		| VERSIONED_READ_STATES
		| VERSIONED_USER_GUILD_SETTINGS
		| DEDUPE_USER_OBJECTS
		| PRIORITIZED_READY_PAYLOAD
		| MULTIPLE_GUILD_EXPERIMENT_POPULATIONS
		| NON_CHANNEL_READ_STATES
		| AUTH_TOKEN_REFRESH
		| USER_SETTINGS_PROTO
		| CLIENT_STATE_V2
		| PASSIVE_GUILD_UPDATE
		| AUTO_CALL_CONNECT
		| DEBOUNCE_MESSAGE_REACTIONS
		| PASSIVE_GUILD_UPDATE_V2;
	pub const DEFAULT_LARGE_THRESHOLD: u32 = 250;

	pub fn new(token: Token, identify: IdentifyProperties) -> Self {
		Self {
			token,
			gateway_url: Self::DEFAULT_GATEWAY_URL.to_owned(),
			identify,
			capabilities: Self::DEFAULT_CAPABILITIES,
			large_threshold: Self::DEFAULT_LARGE_THRESHOLD,
		}
	}
}
