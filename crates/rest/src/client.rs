use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use parking_lot::RwLock;
use protocol::rest::{ApiError, RateLimitBody, SLOWMODE_CODE};
use protocol::{DecodeError, Token};
use reqwest::header::{
	ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap,
	HeaderValue, ORIGIN, REFERER,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::{Host, Url};

use crate::config::RestConfig;
use crate::error::{CaptchaChallenge, RestError, Result, network_error};
use crate::limiter::{RateLimiter, clamp_wait, header_global, header_retry_after, header_u32};
use crate::request::{Body, Request, UploadFile, UploadProgress, encode_component};
use crate::route::{RouteKey, route_key};
use crate::upload::{Meter, metered_body};
use protocol::gateway::{accept_language, sec_ch_ua_platform};
use reqwest::multipart::{Form, Part};
use reqwest::redirect::Policy;
use reqwest::{Body as HttpBody, Method, Response, StatusCode};

const MAX_RETRIES: u32 = 3;
const MAX_AUTO_WAIT: Duration = Duration::from_secs(60);
const RETRY_JITTER: Duration = Duration::from_millis(50);
const FALLBACK_RETRY_AFTER: Duration = Duration::from_secs(1);
const UNSTRUCTURED_RETRY_AFTER: Duration = Duration::from_secs(5);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const WEB_ORIGIN: &str = "https://discord.com";
const WEB_REFERER: &str = "https://discord.com/channels/@me";
const CAPTCHA_MARKER: &[u8] = b"\"captcha_key\"";

struct Throttle {
	retry_after: Duration,
	global: bool,
	timed: bool,
	slowmode: bool,
}

#[derive(Clone)]
pub struct RestClient {
	inner: Arc<Inner>,
}

struct Inner {
	http: reqwest::Client,
	storage: reqwest::Client,
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
	headers.insert("priority", HeaderValue::from_static("u=1, i"));
	headers.insert("x-debug-options", HeaderValue::from_static("bugReporterEnabled"));
	if let Ok(timezone) = HeaderValue::from_str(&host_timezone()) {
		headers.insert("x-discord-timezone", timezone);
	}
	if discord_host(&config.base_url) {
		headers.insert(REFERER, HeaderValue::from_static(WEB_REFERER));
	}
	headers
}

fn host_timezone() -> String {
	iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_owned())
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

fn upload_body(bytes: &Bytes, meter: Option<&Arc<Meter>>) -> HttpBody {
	match meter {
		Some(meter) => metered_body(bytes.clone(), Arc::clone(meter)),
		None => HttpBody::from(bytes.clone()),
	}
}

fn file_part(file: &UploadFile, meter: Option<&Arc<Meter>>) -> Result<Part> {
	Part::stream_with_length(upload_body(&file.bytes, meter), file.bytes.len() as u64)
		.file_name(file.name.clone())
		.mime_str(&file.content_type)
		.map_err(|_| RestError::InvalidRequest)
}

fn multipart_form(
	payload_json: &str,
	files: &[UploadFile],
	progress: Option<&UploadProgress>,
) -> Result<Form> {
	let total = files.iter().map(|file| file.bytes.len() as u64).sum();
	let meter = progress.map(|report| Meter::new(Arc::clone(report), total));
	let payload = Part::text(payload_json.to_owned())
		.mime_str("application/json")
		.map_err(|_| RestError::InvalidRequest)?;
	let mut form = Form::new().part("payload_json", payload);
	for (index, file) in files.iter().enumerate() {
		form = form.part(format!("files[{index}]"), file_part(file, meter.as_ref())?);
	}
	Ok(form)
}

fn fields_form(fields: &[(String, String)], files: &[(String, UploadFile)]) -> Result<Form> {
	let mut form = Form::new();
	for (name, value) in fields {
		form = form.text(name.clone(), value.clone());
	}
	for (name, file) in files {
		form = form.part(name.clone(), file_part(file, None)?);
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

fn contains(body: &[u8], marker: &[u8]) -> bool {
	body.windows(marker.len()).any(|window| window == marker)
}

fn classify(status: StatusCode, body: &[u8]) -> RestError {
	if status == StatusCode::BAD_REQUEST && contains(body, CAPTCHA_MARKER) {
		let value = serde_json::from_slice::<Value>(body).unwrap_or_default();
		let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
		return RestError::CaptchaRequired(Box::new(CaptchaChallenge {
			sitekey: text("captcha_sitekey"),
			service: text("captcha_service"),
			session_id: text("captcha_session_id"),
			rqdata: text("captcha_rqdata"),
			rqtoken: text("captcha_rqtoken"),
			invisible: value
				.get("should_serve_invisible")
				.and_then(Value::as_bool)
				.unwrap_or(false),
		}));
	}
	let value = serde_json::from_slice::<Value>(body).ok();
	let gated = matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN);
	if gated && value.as_ref().is_some_and(|value| value.get("mfa").is_some()) {
		return RestError::MfaRequired;
	}
	let api = value.and_then(|value| serde_json::from_value::<ApiError>(value).ok());
	if api.as_ref().is_some_and(|api| api.code == SLOWMODE_CODE) {
		return RestError::Slowmode { retry_after: None };
	}
	match status {
		StatusCode::UNAUTHORIZED => RestError::Unauthorized(api),
		StatusCode::FORBIDDEN => RestError::Forbidden(api),
		StatusCode::NOT_FOUND => RestError::NotFound(api),
		_ => RestError::Http(status, api),
	}
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
	serde_json::from_slice(body)
		.map_err(|error| RestError::Decode(DecodeError::new(std::any::type_name::<T>(), error)))
}

fn indexing_wait(body: &[u8]) -> Duration {
	serde_json::from_slice::<RateLimitBody>(body)
		.ok()
		.and_then(|parsed| clamp_wait(parsed.retry_after))
		.unwrap_or(FALLBACK_RETRY_AFTER)
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
		let storage = reqwest::Client::builder()
			.redirect(Policy::none())
			.timeout(UPLOAD_TIMEOUT)
			.user_agent(config.user_agent.as_str())
			.build()
			.expect("storage client initializes");
		Self {
			inner: Arc::new(Inner {
				http,
				storage,
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
		let (_, body) = self.execute(request).await?;
		decode(&body)
	}

	pub async fn send_indexed(&self, request: Request) -> Result<Value> {
		let (status, body) = self.execute(request).await?;
		if status == StatusCode::ACCEPTED {
			return Err(RestError::IndexNotReady {
				retry_after: indexing_wait(&body),
			});
		}
		decode(&body)
	}

	pub async fn send_empty(&self, request: Request) -> Result<()> {
		self.execute(request).await.map(drop)
	}

	pub async fn put_upload(
		&self,
		upload_url: &str,
		file: &UploadFile,
		progress: Option<UploadProgress>,
	) -> Result<()> {
		let url = Url::parse(upload_url).map_err(|_| RestError::InvalidRequest)?;
		if !secure_scheme(&url) {
			return Err(RestError::InvalidRequest);
		}
		let meter = progress.map(|report| Meter::new(report, file.bytes.len() as u64));
		let mut builder = self
			.inner
			.storage
			.put(url)
			.header(CONTENT_LENGTH, file.bytes.len())
			.body(upload_body(&file.bytes, meter.as_ref()));
		if discord_host(&self.inner.base_url) {
			builder = builder.header(ORIGIN, HeaderValue::from_static(WEB_ORIGIN));
		}
		let response = builder.send().await.map_err(network_error)?;
		let status = response.status();
		if status.is_success() {
			return Ok(());
		}
		let body = read_capped(response, self.inner.max_body).await?;
		Err(classify(status, &body))
	}

	pub async fn send_bytes(&self, request: Request) -> Result<Bytes> {
		self.execute(request).await.map(|(_, body)| body)
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
		for (name, value) in &request.headers {
			if let Ok(value) = HeaderValue::from_str(value) {
				builder = builder.header(name.as_str(), value);
			}
		}
		builder = match &request.body {
			Body::Empty => builder,
			Body::Json(bytes) => builder
				.header(CONTENT_TYPE, "application/json")
				.body(bytes.clone()),
			Body::Multipart {
				payload_json,
				files,
				progress,
			} => builder.timeout(UPLOAD_TIMEOUT).multipart(multipart_form(
				payload_json,
				files,
				progress.as_ref(),
			)?),
			Body::Form { fields, files } => builder
				.timeout(UPLOAD_TIMEOUT)
				.multipart(fields_form(fields, files)?),
			Body::Invalid => return Err(RestError::InvalidRequest),
		};
		builder.send().await.map_err(network_error)
	}

	fn throttle(&self, route: &RouteKey, headers: &HeaderMap, body: &[u8]) -> Throttle {
		let parsed = serde_json::from_slice::<RateLimitBody>(body).ok();
		let structured = parsed.as_ref().is_some_and(|parsed| parsed.retry_after > 0.0)
			|| headers.contains_key("x-ratelimit-bucket");
		let global =
			!structured || header_global(headers) || parsed.as_ref().is_some_and(|parsed| parsed.global);
		let slowmode = parsed.as_ref().is_some_and(RateLimitBody::is_slowmode);
		let header_wait = header_retry_after(headers);
		let retry_after = parsed
			.filter(|parsed| parsed.retry_after > 0.0)
			.and_then(|parsed| clamp_wait(parsed.retry_after))
			.or(header_wait)
			.unwrap_or(if structured {
				FALLBACK_RETRY_AFTER
			} else {
				UNSTRUCTURED_RETRY_AFTER
			});
		if !slowmode {
			self.inner.limiter.limited(route, retry_after, global);
		}
		Throttle {
			retry_after,
			global,
			timed: structured || header_wait.is_some(),
			slowmode,
		}
	}

	async fn execute(&self, request: Request) -> Result<(StatusCode, Bytes)> {
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
				let throttle = self.throttle(&route, &headers, &body);
				let retry_after = throttle.retry_after;
				if throttle.slowmode {
					return Err(RestError::Slowmode {
						retry_after: Some(retry_after),
					});
				}
				if !retryable || !throttle.timed || retries >= MAX_RETRIES || retry_after > MAX_AUTO_WAIT {
					return Err(RestError::RateLimited { retry_after });
				}
				retries += 1;
				tracing::debug!(route = %route, global = throttle.global, retries, "rest rate limited, retrying");
				tokio::time::sleep(retry_after + RETRY_JITTER).await;
				continue;
			}
			if status.is_success() {
				return Ok((status, body));
			}
			return Err(classify(status, &body));
		}
	}
}
