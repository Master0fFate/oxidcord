use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use parking_lot::RwLock;
use protocol::rest::{ApiError, RateLimitBody};
use protocol::{DecodeError, Token};
use reqwest::header::{
	ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, ORIGIN,
	REFERER,
};
use serde::de::DeserializeOwned;
use url::{Host, Url};

use crate::config::RestConfig;
use crate::error::{RestError, Result, network_error};
use crate::limiter::{RateLimiter, header_global, header_retry_after, header_u32};
use crate::request::{Body, Request, UploadFile, encode_component};
use crate::route::{RouteKey, route_key};
use protocol::gateway::{accept_language, sec_ch_ua_platform};
use reqwest::multipart::{Form, Part};
use reqwest::redirect::Policy;
use reqwest::{Body as HttpBody, Method, Response, StatusCode};

const MAX_RETRIES: u32 = 3;
const MAX_AUTO_WAIT: Duration = Duration::from_secs(60);
const RETRY_JITTER: Duration = Duration::from_millis(50);
const FALLBACK_RETRY_AFTER: Duration = Duration::from_secs(1);
const WEB_ORIGIN: &str = "https://discord.com";
const WEB_REFERER: &str = "https://discord.com/channels/@me";

#[derive(Clone)]
pub struct RestClient {
	inner: Arc<Inner>,
}

struct Inner {
	http: reqwest::Client,
	base_url: Url,
	max_body: usize,
	token: Arc<RwLock<Option<Token>>>,
	limiter: RateLimiter,
	frecency: tokio::sync::Mutex<()>,
}

fn default_headers(config: &RestConfig) -> HeaderMap {
	let mut headers = HeaderMap::new();
	headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
	headers.insert(
		ACCEPT_ENCODING,
		HeaderValue::from_static("gzip, deflate, br, zstd"),
	);
	if let Ok(language) = HeaderValue::from_str(&accept_language(&config.locale)) {
		headers.insert(ACCEPT_LANGUAGE, language);
	}
	if let Ok(locale) = HeaderValue::from_str(&config.locale) {
		headers.insert("x-discord-locale", locale);
	}
	if let Some(properties) = &config.super_properties
		&& let Ok(properties) = HeaderValue::from_str(properties)
	{
		headers.insert("x-super-properties", properties);
	}
	if let Some(sec_ch_ua) = &config.sec_ch_ua
		&& let Ok(value) = HeaderValue::from_str(sec_ch_ua)
	{
		headers.insert("sec-ch-ua", value);
	}
	headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
	headers.insert(
		"sec-ch-ua-platform",
		HeaderValue::from_static(sec_ch_ua_platform()),
	);
	headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
	headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
	headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
	if discord_host(&config.base_url) {
		headers.insert(REFERER, HeaderValue::from_static(WEB_REFERER));
	}
	headers
}

fn discord_host(url: &Url) -> bool {
	url.scheme() == "https"
		&& url
			.host_str()
			.is_some_and(|host| host == "discord.com" || host.ends_with(".discord.com"))
}

fn secure_scheme(url: &Url) -> bool {
	match url.scheme() {
		"https" => true,
		"http" => match url.host() {
			Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
			Some(Host::Ipv4(address)) => address.is_loopback(),
			Some(Host::Ipv6(address)) => address.is_loopback(),
			None => false,
		},
		_ => false,
	}
}

fn file_part(file: &UploadFile) -> Result<Part> {
	Part::stream_with_length(HttpBody::from(file.bytes.clone()), file.bytes.len() as u64)
		.file_name(file.name.clone())
		.mime_str(&file.content_type)
		.map_err(|_| RestError::InvalidRequest)
}

fn multipart_form(payload_json: &str, files: &[UploadFile]) -> Result<Form> {
	let payload = Part::text(payload_json.to_owned())
		.mime_str("application/json")
		.map_err(|_| RestError::InvalidRequest)?;
	let mut form = Form::new().part("payload_json", payload);
	for (index, file) in files.iter().enumerate() {
		form = form.part(format!("files[{index}]"), file_part(file)?);
	}
	Ok(form)
}

fn fields_form(fields: &[(String, String)], files: &[(String, UploadFile)]) -> Result<Form> {
	let mut form = Form::new();
	for (name, value) in fields {
		form = form.text(name.clone(), value.clone());
	}
	for (name, file) in files {
		form = form.part(name.clone(), file_part(file)?);
	}
	Ok(form)
}

async fn read_capped(response: Response, max: usize) -> Result<Bytes> {
	if response
		.content_length()
		.is_some_and(|length| length > max as u64)
	{
		return Err(RestError::TooLarge);
	}
	let capacity = response.content_length().map_or(0, |length| length as usize);
	let mut buffer = BytesMut::with_capacity(capacity.min(max));
	let mut stream = response.bytes_stream();
	while let Some(chunk) = stream.next().await {
		let chunk = chunk.map_err(network_error)?;
		if buffer.len() + chunk.len() > max {
			return Err(RestError::TooLarge);
		}
		buffer.extend_from_slice(&chunk);
	}
	Ok(buffer.freeze())
}

fn classify(status: StatusCode, body: &[u8]) -> RestError {
	match status {
		StatusCode::UNAUTHORIZED => RestError::Unauthorized,
		StatusCode::FORBIDDEN => RestError::Forbidden,
		StatusCode::NOT_FOUND => RestError::NotFound,
		_ => RestError::Http(status, serde_json::from_slice::<ApiError>(body).ok()),
	}
}

impl RestClient {
	pub fn new(config: RestConfig) -> Self {
		let http = reqwest::Client::builder()
			.redirect(Policy::none())
			.timeout(config.timeout)
			.user_agent(config.user_agent.as_str())
			.default_headers(default_headers(&config))
			.build()
			.expect("http client initializes");
		Self {
			inner: Arc::new(Inner {
				http,
				base_url: config.base_url,
				max_body: config.max_body,
				token: Arc::default(),
				limiter: RateLimiter::default(),
				frecency: tokio::sync::Mutex::new(()),
			}),
		}
	}

	pub fn set_token(&self, token: Token) {
		*self.inner.token.write() = Some(token);
	}

	pub fn clear_token(&self) {
		*self.inner.token.write() = None;
	}

	pub(crate) async fn frecency_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
		self.inner.frecency.lock().await
	}

	pub async fn send<T: DeserializeOwned>(&self, request: Request) -> Result<T> {
		let body = self.execute(request).await?;
		serde_json::from_slice(&body)
			.map_err(|error| RestError::Decode(DecodeError::new(std::any::type_name::<T>(), error)))
	}

	pub async fn send_empty(&self, request: Request) -> Result<()> {
		self.execute(request).await.map(drop)
	}

	pub async fn send_bytes(&self, request: Request) -> Result<Bytes> {
		self.execute(request).await
	}

	fn build_url(&self, request: &Request) -> Result<Url> {
		let base = self.inner.base_url.as_str().trim_end_matches('/');
		let mut url =
			Url::parse(&format!("{base}{}", request.path)).map_err(|_| RestError::InvalidRequest)?;
		if !request.query.is_empty() {
			url.query_pairs_mut().extend_pairs(&request.query);
		}
		Ok(url)
	}

	fn authorization(&self, url: &Url) -> Option<HeaderValue> {
		let base = &self.inner.base_url;
		if !secure_scheme(url)
			|| url.host_str() != base.host_str()
			|| url.port_or_known_default() != base.port_or_known_default()
		{
			return None;
		}
		let guard = self.inner.token.read();
		let mut value = HeaderValue::from_str(guard.as_ref()?.as_str()).ok()?;
		value.set_sensitive(true);
		Some(value)
	}

	async fn dispatch(&self, request: &Request, url: &Url) -> Result<Response> {
		let mut builder = self.inner.http.request(request.method.clone(), url.clone());
		if let Some(authorization) = self.authorization(url) {
			builder = builder.header(AUTHORIZATION, authorization);
		}
		if let Some(reason) = &request.audit_reason {
			builder = builder.header("x-audit-log-reason", encode_component(reason));
		}
		if request.method != Method::GET && discord_host(url) {
			builder = builder.header(ORIGIN, HeaderValue::from_static(WEB_ORIGIN));
		}
		builder = match &request.body {
			Body::Empty => builder,
			Body::Json(bytes) => builder
				.header(CONTENT_TYPE, "application/json")
				.body(bytes.clone()),
			Body::Multipart { payload_json, files } => {
				builder.multipart(multipart_form(payload_json, files)?)
			}
			Body::Form { fields, files } => builder.multipart(fields_form(fields, files)?),
			Body::Invalid => return Err(RestError::InvalidRequest),
		};
		builder.send().await.map_err(network_error)
	}

	fn rate_limit_wait(&self, route: &RouteKey, headers: &HeaderMap, body: &[u8]) -> (Duration, bool) {
		let parsed = serde_json::from_slice::<RateLimitBody>(body).ok();
		let global = header_global(headers) || parsed.as_ref().is_some_and(|parsed| parsed.global);
		let retry_after = parsed
			.and_then(|parsed| Duration::try_from_secs_f64(parsed.retry_after.max(0.0)).ok())
			.or_else(|| header_retry_after(headers))
			.unwrap_or(FALLBACK_RETRY_AFTER);
		self.inner.limiter.limited(route, retry_after, global);
		(retry_after, global)
	}

	async fn execute(&self, request: Request) -> Result<Bytes> {
		let url = self.build_url(&request)?;
		let route = route_key(&request.method, &request.path);
		let retryable = request.method != Method::POST || request.idempotent;
		let mut retries = 0;
		loop {
			self.inner.limiter.acquire(&route).await;
			let started = Instant::now();
			let response = self.dispatch(&request, &url).await?;
			let status = response.status();
			let headers = response.headers().clone();
			self.inner.limiter.observe(&route, &headers);
			tracing::debug!(
				method = %request.method,
				route = %route,
				status = status.as_u16(),
				remaining = ?header_u32(&headers, "x-ratelimit-remaining"),
				elapsed_ms = started.elapsed().as_millis() as u64,
				"rest response"
			);
			if status.is_redirection() {
				return Err(RestError::Redirect);
			}
			let body = read_capped(response, self.inner.max_body).await?;
			if status == StatusCode::TOO_MANY_REQUESTS {
				let (retry_after, global) = self.rate_limit_wait(&route, &headers, &body);
				if !retryable || retries >= MAX_RETRIES || retry_after > MAX_AUTO_WAIT {
					return Err(RestError::RateLimited { retry_after });
				}
				retries += 1;
				tracing::debug!(route = %route, global, retries, "rest rate limited, retrying");
				tokio::time::sleep(retry_after + RETRY_JITTER).await;
				continue;
			}
			if status.is_success() {
				return Ok(body);
			}
			return Err(classify(status, &body));
		}
	}
}
