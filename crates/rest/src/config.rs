use std::time::Duration;

use protocol::gateway::{CHROME_VERSION, FALLBACK_BUILD_NUMBER, IdentifyProperties, sec_ch_ua};
use url::Url;

const DEFAULT_BASE_URL: &str = "https://discord.com/api/v10";
const DEFAULT_LOCALE: &str = "en-US";

#[derive(Debug, Clone)]
pub struct RestConfig {
	pub base_url: Url,
	pub user_agent: String,
	pub locale: String,
	pub super_properties: Option<String>,
	pub sec_ch_ua: Option<String>,
	pub timeout: Duration,
	pub max_body: usize,
}

impl RestConfig {
	pub fn with_identity(mut self, identify: &IdentifyProperties) -> Self {
		self.user_agent = identify.user_agent().to_owned();
		self.super_properties = Some(identify.super_properties());
		self.sec_ch_ua = identify.browser_version.as_deref().map(sec_ch_ua);
		self
	}
}

impl Default for RestConfig {
	fn default() -> Self {
		let identify = IdentifyProperties::web_client(DEFAULT_LOCALE, FALLBACK_BUILD_NUMBER, CHROME_VERSION);
		Self {
			base_url: Url::parse(DEFAULT_BASE_URL).expect("default base url is valid"),
			user_agent: String::new(),
			locale: DEFAULT_LOCALE.to_owned(),
			super_properties: None,
			sec_ch_ua: None,
			timeout: Duration::from_secs(30),
			max_body: 16 * 1024 * 1024,
		}
		.with_identity(&identify)
	}
}
