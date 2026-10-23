use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::redirect::Policy;
use reqwest::{Client, ClientBuilder, Response};
use url::Url;

use crate::error::{Result, UpdaterError};

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REDIRECTS: usize = 5;
const GITHUB_API: &str = "https://api.github.com/";
const GITHUB_HOSTS: [&str; 4] = [
	"api.github.com",
	"github.com",
	"objects.githubusercontent.com",
	"release-assets.githubusercontent.com",
];

#[derive(Clone)]
pub(crate) struct UrlPolicy {
	hosts: Arc<[String]>,
	relaxed: bool,
}

impl UrlPolicy {
	fn github() -> Self {
		Self {
			hosts: GITHUB_HOSTS.iter().map(|host| (*host).to_owned()).collect(),
			relaxed: false,
		}
	}

	fn local(base: &Url) -> Result<Self> {
		let host = base.host_str().ok_or(UpdaterError::UnsafeUrl)?;
		Ok(Self {
			hosts: Arc::from([host.to_owned()]),
			relaxed: true,
		})
	}

	pub(crate) fn check(&self, url: &Url) -> Result<()> {
		let scheme_ok = url.scheme() == "https" || (self.relaxed && url.scheme() == "http");
		let port_ok = self.relaxed || url.port().is_none();
		let host_ok = url
			.host_str()
			.is_some_and(|host| self.hosts.iter().any(|allowed| allowed == host));
		let anonymous = url.username().is_empty() && url.password().is_none();
		if scheme_ok && port_ok && host_ok && anonymous {
			Ok(())
		} else {
			Err(UpdaterError::UnsafeUrl)
		}
	}
}

pub(crate) struct Net {
	api: Client,
	files: Client,
	base: Url,
	policy: UrlPolicy,
}

impl Net {
	pub(crate) fn github() -> Result<Self> {
		let base = Url::parse(GITHUB_API).map_err(|_| UpdaterError::UnsafeUrl)?;
		Self::build(base, UrlPolicy::github())
	}

	pub(crate) fn local(base: Url) -> Result<Self> {
		let policy = UrlPolicy::local(&base)?;
		Self::build(base, policy)
	}

	fn build(base: Url, policy: UrlPolicy) -> Result<Self> {
		let api = client(&policy, |builder| builder.timeout(TIMEOUT))?;
		let files = client(&policy, |builder| {
			builder.connect_timeout(TIMEOUT).read_timeout(TIMEOUT)
		})?;
		Ok(Self {
			api,
			files,
			base,
			policy,
		})
	}

	pub(crate) fn api_url(&self, path: &str) -> Result<Url> {
		self.base.join(path).map_err(|_| UpdaterError::UnsafeUrl)
	}

	pub(crate) fn parse_url(&self, text: &str) -> Result<Url> {
		let url = Url::parse(text).map_err(|_| UpdaterError::UnsafeUrl)?;
		self.policy.check(&url)?;
		Ok(url)
	}

	pub(crate) async fn get_capped(&self, url: &Url, limit: u64) -> Result<Vec<u8>> {
		let response = self.get(&self.api, url).await?;
		if response.content_length().is_some_and(|length| length > limit) {
			return Err(UpdaterError::TooLarge { limit });
		}
		let mut body = Vec::new();
		let mut stream = response.bytes_stream();
		while let Some(chunk) = stream.next().await {
			let chunk = chunk?;
			if body.len() as u64 + chunk.len() as u64 > limit {
				return Err(UpdaterError::TooLarge { limit });
			}
			body.extend_from_slice(&chunk);
		}
		Ok(body)
	}

	pub(crate) async fn get_file(&self, url: &Url) -> Result<Response> {
		self.get(&self.files, url).await
	}

	async fn get(&self, client: &Client, url: &Url) -> Result<Response> {
		self.policy.check(url)?;
		let response = client.get(url.clone()).send().await?;
		let status = response.status();
		if status.is_success() {
			Ok(response)
		} else {
			Err(UpdaterError::Status(status.as_u16()))
		}
	}
}

fn client(policy: &UrlPolicy, configure: impl FnOnce(ClientBuilder) -> ClientBuilder) -> Result<Client> {
	let redirect_policy = policy.clone();
	let builder = Client::builder()
		.user_agent(concat!("oxidcord-updater/", env!("CARGO_PKG_VERSION")))
		.https_only(!policy.relaxed)
		.referer(false)
		.redirect(Policy::custom(move |attempt| {
			if attempt.previous().len() >= MAX_REDIRECTS {
				attempt.error("too many redirects")
			} else if redirect_policy.check(attempt.url()).is_ok() {
				attempt.follow()
			} else {
				attempt.error("redirect target is not allowed")
			}
		}));
	Ok(configure(builder).build()?)
}
