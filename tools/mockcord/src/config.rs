const ROTATED_SUFFIX: &str = "-rotated";

#[derive(Debug, Clone)]
pub struct MockConfig {
	pub port: Option<u16>,
	pub seed: u64,
	pub guilds: usize,
	pub channels_per_guild: usize,
	pub messages_per_channel: usize,
	pub members_per_guild: usize,
	pub token: String,
	pub heartbeat_interval_ms: u64,
	pub chaos: bool,
	pub live_traffic: bool,
}

impl MockConfig {
	pub fn rotated_token(&self) -> String {
		format!("{}{ROTATED_SUFFIX}", self.token)
	}

	pub fn accepts(&self, token: &str) -> bool {
		token == self.token || token.strip_prefix(self.token.as_str()) == Some(ROTATED_SUFFIX)
	}
}

impl Default for MockConfig {
	fn default() -> Self {
		Self {
			port: None,
			seed: 1,
			guilds: 3,
			channels_per_guild: 8,
			messages_per_channel: 200,
			members_per_guild: 50,
			token: "mock-token".to_owned(),
			heartbeat_interval_ms: 41_250,
			chaos: false,
			live_traffic: false,
		}
	}
}
