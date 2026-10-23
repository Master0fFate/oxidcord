use protocol::{IdentifyProperties, Token};

#[derive(Debug)]
pub struct GatewayConfig {
	pub token: Token,
	pub gateway_url: String,
	pub identify: IdentifyProperties,
	pub capabilities: u64,
	pub initial_status: String,
	pub large_threshold: u32,
}

impl GatewayConfig {
	pub const DEFAULT_GATEWAY_URL: &'static str = "wss://gateway.discord.gg";
	pub const DEFAULT_CAPABILITIES: u64 =
		1 | 2 | 4 | 8 | 16 | 32 | 64 | 128 | 256 | 512 | 1024 | 2048 | 4096 | 8192 | 16384;
	pub const DEFAULT_LARGE_THRESHOLD: u32 = 250;

	pub fn new(token: Token, identify: IdentifyProperties) -> Self {
		Self {
			token,
			gateway_url: Self::DEFAULT_GATEWAY_URL.to_owned(),
			identify,
			capabilities: Self::DEFAULT_CAPABILITIES,
			initial_status: "online".to_owned(),
			large_threshold: Self::DEFAULT_LARGE_THRESHOLD,
		}
	}
}
