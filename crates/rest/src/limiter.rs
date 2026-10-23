use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use reqwest::header::HeaderMap;

use crate::route::RouteKey;

const MAX_HEADER_WAIT: f64 = 600.0;
const PRUNE_THRESHOLD: usize = 1024;

#[derive(Default)]
pub struct RateLimiter {
	state: Mutex<State>,
}

#[derive(Default)]
struct State {
	route_buckets: HashMap<String, String>,
	buckets: HashMap<String, Bucket>,
	global_until: Option<Instant>,
}

struct Bucket {
	limit: u32,
	remaining: u32,
	window: Duration,
	reset_at: Instant,
}

fn bucket_id(hash: &str, route: &RouteKey) -> String {
	format!("{hash}:{}", route.major())
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
	headers.get(name)?.to_str().ok()
}

fn header_secs(headers: &HeaderMap, name: &str) -> Option<Duration> {
	let seconds = header_text(headers, name)?.parse::<f64>().ok()?;
	Duration::try_from_secs_f64(seconds.clamp(0.0, MAX_HEADER_WAIT)).ok()
}

pub(crate) fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
	header_text(headers, name)?.parse().ok()
}

pub(crate) fn header_retry_after(headers: &HeaderMap) -> Option<Duration> {
	header_secs(headers, "retry-after")
}

pub(crate) fn header_global(headers: &HeaderMap) -> bool {
	header_text(headers, "x-ratelimit-global").is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

impl RateLimiter {
	pub async fn acquire(&self, route: &RouteKey) {
		while let Some(wait) = self.reserve(route) {
			tokio::time::sleep(wait).await;
		}
	}

	fn reserve(&self, route: &RouteKey) -> Option<Duration> {
		let now = Instant::now();
		let mut guard = self.state.lock();
		let state = &mut *guard;
		if let Some(until) = state.global_until {
			if until > now {
				return Some(until - now);
			}
			state.global_until = None;
		}
		let hash = state.route_buckets.get(route.as_str())?;
		let id = bucket_id(hash, route);
		let bucket = state.buckets.get_mut(&id)?;
		if bucket.reset_at <= now {
			bucket.remaining = bucket.limit;
			bucket.reset_at = now + bucket.window;
		}
		if bucket.remaining == 0 {
			return Some(bucket.reset_at - now);
		}
		bucket.remaining -= 1;
		None
	}

	pub fn observe(&self, route: &RouteKey, headers: &HeaderMap) {
		let Some(hash) = header_text(headers, "x-ratelimit-bucket") else {
			return;
		};
		let (Some(remaining), Some(reset_after)) = (
			header_u32(headers, "x-ratelimit-remaining"),
			header_secs(headers, "x-ratelimit-reset-after"),
		) else {
			return;
		};
		let now = Instant::now();
		let mut state = self.state.lock();
		let id = bucket_id(hash, route);
		if state.route_buckets.get(route.as_str()).map(String::as_str) != Some(hash) {
			state
				.route_buckets
				.insert(route.as_str().to_owned(), hash.to_owned());
		}
		let limit = header_u32(headers, "x-ratelimit-limit")
			.or_else(|| state.buckets.get(&id).map(|bucket| bucket.limit))
			.unwrap_or(remaining)
			.max(remaining);
		state.buckets.insert(
			id,
			Bucket {
				limit,
				remaining,
				window: reset_after,
				reset_at: now + reset_after,
			},
		);
		if state.buckets.len() > PRUNE_THRESHOLD {
			state.buckets.retain(|_, bucket| bucket.reset_at > now);
		}
	}

	pub fn limited(&self, route: &RouteKey, retry_after: Duration, global: bool) {
		let reset_at = Instant::now() + retry_after;
		let mut guard = self.state.lock();
		let state = &mut *guard;
		if global {
			state.global_until = Some(reset_at);
			return;
		}
		let Some(hash) = state.route_buckets.get(route.as_str()) else {
			return;
		};
		let id = bucket_id(hash, route);
		if let Some(bucket) = state.buckets.get_mut(&id) {
			bucket.remaining = 0;
			bucket.reset_at = reset_at;
		}
	}
}
