use std::fmt;

use reqwest::Method;

const MAJOR_PARENTS: [&str; 3] = ["channels", "guilds", "webhooks"];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteKey {
	key: String,
	major: String,
}

impl RouteKey {
	pub fn as_str(&self) -> &str {
		&self.key
	}

	pub fn major(&self) -> &str {
		&self.major
	}
}

impl fmt::Display for RouteKey {
	fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter.write_str(&self.key)
	}
}

fn is_id(segment: &str) -> bool {
	!segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn route_key(method: &Method, path: &str) -> RouteKey {
	let mut key = String::with_capacity(path.len() + 8);
	let mut major = String::new();
	key.push_str(method.as_str());
	key.push(' ');
	let mut previous = "";
	for segment in path.split('/').filter(|segment| !segment.is_empty()) {
		key.push('/');
		if MAJOR_PARENTS.contains(&previous) && is_id(segment) {
			key.push_str(segment);
			if !major.is_empty() {
				major.push(',');
			}
			major.push_str(previous);
			major.push('/');
			major.push_str(segment);
		} else if previous == "reactions" {
			key.push_str("{emoji}");
		} else if previous == "invites" {
			key.push_str("{code}");
		} else if is_id(segment) {
			key.push_str("{id}");
		} else {
			key.push_str(segment);
		}
		previous = segment;
	}
	RouteKey { key, major }
}
