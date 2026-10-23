use futures_util::StreamExt;
use protocol::gateway::{CHROME_VERSION, IdentifyProperties};
use reqwest::Client;
use serde_json::Value;
use url::Url;

use crate::error::{RestError, Result, network_error};

const APP_URL: &str = "https://discord.com/app";
const CHROME_VERSION_URL: &str = "https://chromiumdash.appspot.com/version.json";
const MAX_VERSION_BYTES: usize = 16 * 1024;
const MAX_SCRIPTS: usize = 6;
const MAX_TOTAL_BYTES: usize = 30 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const KEYS: [&[u8]; 2] = [b"build_number:\"", b"buildNumber:\""];

pub fn discovery_client() -> Result<Client> {
	let identify = IdentifyProperties::web_client("en-US", 0, CHROME_VERSION);
	Client::builder()
		.user_agent(identify.user_agent())
		.build()
		.map_err(network_error)
}

pub async fn discover_chrome_version(client: &Client) -> Result<String> {
	let base = Url::parse(CHROME_VERSION_URL).map_err(|_| RestError::InvalidRequest)?;
	let (body, _) = fetch_capped(client, &base, MAX_VERSION_BYTES).await?;
	let parsed: Value = serde_json::from_slice(&body).map_err(|_| RestError::NotFound)?;
	let version = parsed
		.get("latest_version")
		.and_then(|channel| channel.get("stable"))
		.and_then(Value::as_str)
		.ok_or(RestError::NotFound)?
		.to_owned();
	if !plausible_version(&version) {
		return Err(RestError::NotFound);
	}
	Ok(version)
}

fn plausible_version(version: &str) -> bool {
	version.split('.').count() == 4
		&& version
			.split('.')
			.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

pub async fn discover_build_number(client: &Client) -> Result<u64> {
	let base = Url::parse(APP_URL).map_err(|_| RestError::InvalidRequest)?;
	let (page, _) = fetch_capped(client, &base, MAX_PAGE_BYTES).await?;
	let scripts = script_urls(&base, &String::from_utf8_lossy(&page));
	let mut remaining = MAX_TOTAL_BYTES;
	for url in scripts.iter().rev().take(MAX_SCRIPTS) {
		let (body, complete) = fetch_capped(client, url, remaining).await?;
		remaining -= body.len();
		if let Some(number) = find_build_number(&body) {
			return Ok(number);
		}
		if !complete || remaining == 0 {
			break;
		}
	}
	Err(RestError::NotFound)
}

async fn fetch_capped(client: &Client, url: &Url, cap: usize) -> Result<(Vec<u8>, bool)> {
	let response = client
		.get(url.clone())
		.send()
		.await
		.map_err(network_error)?
		.error_for_status()
		.map_err(network_error)?;
	let mut body = Vec::new();
	let mut stream = response.bytes_stream();
	while let Some(chunk) = stream.next().await {
		let chunk = chunk.map_err(network_error)?;
		let room = cap - body.len();
		if chunk.len() > room {
			body.extend_from_slice(&chunk[..room]);
			return Ok((body, false));
		}
		body.extend_from_slice(&chunk);
	}
	Ok((body, true))
}

fn script_urls(base: &Url, html: &str) -> Vec<Url> {
	html.split("src=\"")
		.skip(1)
		.filter_map(|rest| rest.split('"').next())
		.filter(|source| source.starts_with("/assets/") && source.ends_with(".js"))
		.filter_map(|source| base.join(source).ok())
		.collect()
}

fn find_build_number(body: &[u8]) -> Option<u64> {
	KEYS.iter().find_map(|key| {
		body.windows(key.len())
			.enumerate()
			.filter(|(_, window)| window == key)
			.find_map(|(index, _)| {
				let digits = &body[index + key.len()..];
				let length = digits.iter().take_while(|byte| byte.is_ascii_digit()).count();
				let closed = digits.get(length) == Some(&b'"');
				let text = std::str::from_utf8(&digits[..length]).ok()?;
				closed.then(|| text.parse().ok()).flatten()
			})
	})
}
