use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use reqwest::{Client, Method, StatusCode};
use tokio::runtime::Handle;
use url::{Host, Url};

use crate::api::{FetchRequest, FetchResponse, HostError};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const MAX_HEADERS: usize = 32;
const HTTPS_PORT: u16 = 443;

const FORBIDDEN_REQUEST_HEADERS: [&str; 10] = [
	"host",
	"content-length",
	"transfer-encoding",
	"connection",
	"cookie",
	"proxy-authorization",
	"proxy-connection",
	"upgrade",
	"te",
	"expect",
];

pub struct Fetcher {
	handle: Handle,
}

fn invalid(reason: &str) -> HostError {
	HostError::Invalid(reason.to_owned())
}

impl Fetcher {
	pub fn new(handle: Handle) -> Self {
		Self { handle }
	}

	pub fn fetch(
		&self,
		request: FetchRequest,
		host_allowed: impl Fn(&str) -> bool,
	) -> Result<FetchResponse, HostError> {
		let prepared = Prepared::from_request(request)?;
		self.handle.block_on(async {
			tokio::time::timeout(REQUEST_TIMEOUT, run(prepared, host_allowed))
				.await
				.map_err(|_| HostError::Timeout)?
		})
	}
}

struct Prepared {
	method: Method,
	url: Url,
	headers: HeaderMap,
	body: Option<Vec<u8>>,
}

impl Prepared {
	fn from_request(request: FetchRequest) -> Result<Self, HostError> {
		let method = Method::from_bytes(request.method.to_ascii_uppercase().as_bytes())
			.ok()
			.filter(|method| {
				matches!(
					*method,
					Method::GET | Method::HEAD | Method::POST | Method::PUT | Method::PATCH | Method::DELETE
				)
			})
			.ok_or_else(|| invalid("unsupported method"))?;
		let url = Url::parse(&request.url).map_err(|_| invalid("invalid url"))?;
		if request.headers.len() > MAX_HEADERS {
			return Err(invalid("too many headers"));
		}
		let mut headers = HeaderMap::new();
		for (name, value) in &request.headers {
			let name = HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes())
				.map_err(|_| invalid("invalid header name"))?;
			if FORBIDDEN_REQUEST_HEADERS.contains(&name.as_str()) {
				return Err(invalid("forbidden header"));
			}
			let value = HeaderValue::from_str(value).map_err(|_| invalid("invalid header value"))?;
			headers.append(name, value);
		}
		if request
			.body
			.as_ref()
			.is_some_and(|body| body.len() > MAX_REQUEST_BYTES)
		{
			return Err(invalid("request body too large"));
		}
		Ok(Self {
			method,
			url,
			headers,
			body: request.body,
		})
	}
}

async fn run(prepared: Prepared, host_allowed: impl Fn(&str) -> bool) -> Result<FetchResponse, HostError> {
	let Prepared {
		mut method,
		mut url,
		headers,
		mut body,
	} = prepared;
	for _ in 0..=MAX_REDIRECTS {
		let client = client_for(&url, &host_allowed).await?;
		let mut request = client
			.request(method.clone(), url.clone())
			.headers(headers.clone());
		if let Some(body) = &body {
			request = request.body(body.clone());
		}
		let response = request.send().await.map_err(map_error)?;
		let status = response.status();
		if status.is_redirection()
			&& let Some(location) = response.headers().get(reqwest::header::LOCATION)
		{
			let location = location.to_str().map_err(|_| invalid("invalid redirect"))?;
			url = url.join(location).map_err(|_| invalid("invalid redirect"))?;
			if redirect_drops_body(status, &method) {
				method = Method::GET;
				body = None;
			}
			continue;
		}
		return read_response(response).await;
	}
	Err(invalid("too many redirects"))
}

fn redirect_drops_body(status: StatusCode, method: &Method) -> bool {
	matches!(
		status,
		StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER
	) && method != Method::GET
		&& method != Method::HEAD
}

async fn client_for(url: &Url, host_allowed: &impl Fn(&str) -> bool) -> Result<Client, HostError> {
	if url.scheme() != "https" {
		return Err(invalid("only https is allowed"));
	}
	if !url.username().is_empty() || url.password().is_some() {
		return Err(invalid("credentials in url are not allowed"));
	}
	if url.port().is_some_and(|port| port != HTTPS_PORT) {
		return Err(invalid("only port 443 is allowed"));
	}
	let host = url.host().ok_or_else(|| invalid("url has no host"))?;
	let host_text = host.to_string();
	let bare_host = host_text.trim_start_matches('[').trim_end_matches(']').to_owned();
	if !host_allowed(&bare_host) {
		return Err(invalid("host is not in the approved list"));
	}
	let builder = Client::builder()
		.redirect(Policy::none())
		.https_only(true)
		.no_proxy()
		.referer(false)
		.timeout(REQUEST_TIMEOUT);
	let builder = match host {
		Host::Ipv4(address) => {
			ensure_public(IpAddr::V4(address))?;
			builder
		}
		Host::Ipv6(address) => {
			ensure_public(IpAddr::V6(address))?;
			builder
		}
		Host::Domain(domain) => {
			let addresses = resolve_public(domain).await?;
			builder.resolve_to_addrs(domain, &addresses)
		}
	};
	builder.build().map_err(map_error)
}

async fn resolve_public(domain: &str) -> Result<Vec<SocketAddr>, HostError> {
	let addresses: Vec<SocketAddr> = tokio::net::lookup_host((domain, HTTPS_PORT))
		.await
		.map_err(|_| invalid("dns lookup failed"))?
		.collect();
	if addresses.is_empty() {
		return Err(invalid("dns lookup returned no addresses"));
	}
	for address in &addresses {
		ensure_public(address.ip())?;
	}
	Ok(addresses)
}

fn ensure_public(address: IpAddr) -> Result<(), HostError> {
	if is_public(address) {
		Ok(())
	} else {
		Err(invalid("address is not publicly routable"))
	}
}

pub fn is_public(address: IpAddr) -> bool {
	match address {
		IpAddr::V4(address) => is_public_v4(address),
		IpAddr::V6(address) => is_public_v6(address),
	}
}

fn is_public_v4(address: Ipv4Addr) -> bool {
	let octets = address.octets();
	!(address.is_loopback()
		|| address.is_private()
		|| address.is_link_local()
		|| address.is_multicast()
		|| address.is_unspecified()
		|| address.is_broadcast()
		|| octets[0] == 0
		|| (octets[0] == 100 && (octets[1] & 0xc0) == 64)
		|| (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
		|| octets[0] >= 240)
}

fn is_public_v6(address: Ipv6Addr) -> bool {
	if let Some(mapped) = address.to_ipv4_mapped() {
		return is_public_v4(mapped);
	}
	let segments = address.segments();
	if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
		let [high, low] = [segments[6], segments[7]];
		let embedded = Ipv4Addr::new((high >> 8) as u8, high as u8, (low >> 8) as u8, low as u8);
		return is_public_v4(embedded);
	}
	!(address.is_loopback()
		|| address.is_unspecified()
		|| address.is_multicast()
		|| (segments[0] & 0xffc0) == 0xfe80
		|| (segments[0] & 0xfe00) == 0xfc00)
}

async fn read_response(mut response: reqwest::Response) -> Result<FetchResponse, HostError> {
	if response
		.content_length()
		.is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
	{
		return Err(invalid("response too large"));
	}
	let status = response.status().as_u16();
	let headers = response
		.headers()
		.iter()
		.filter(|(name, _)| *name != reqwest::header::SET_COOKIE)
		.filter_map(|(name, value)| Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned())))
		.collect();
	let mut body = Vec::new();
	while let Some(chunk) = response.chunk().await.map_err(map_error)? {
		if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
			return Err(invalid("response too large"));
		}
		body.extend_from_slice(&chunk);
	}
	Ok(FetchResponse {
		status,
		headers,
		body,
	})
}

fn map_error(error: reqwest::Error) -> HostError {
	if error.is_timeout() {
		HostError::Timeout
	} else {
		HostError::Invalid(error.without_url().to_string())
	}
}
