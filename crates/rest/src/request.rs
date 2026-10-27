use bytes::Bytes;
use reqwest::Method;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct UploadFile {
	pub name: String,
	pub bytes: Bytes,
	pub content_type: String,
	pub description: Option<String>,
}

pub(crate) enum Body {
	Empty,
	Json(Bytes),
	Multipart {
		payload_json: String,
		files: Vec<UploadFile>,
	},
	Form {
		fields: Vec<(String, String)>,
		files: Vec<(String, UploadFile)>,
	},
	Invalid,
}

pub struct Request {
	pub(crate) method: Method,
	pub(crate) path: String,
	pub(crate) query: Vec<(String, String)>,
	pub(crate) audit_reason: Option<String>,
	pub(crate) idempotent: bool,
	pub(crate) body: Body,
}

impl Request {
	fn new(method: Method, path: impl Into<String>) -> Self {
		Self {
			method,
			path: path.into(),
			query: Vec::new(),
			audit_reason: None,
			idempotent: false,
			body: Body::Empty,
		}
	}

	pub fn get(path: impl Into<String>) -> Self {
		Self::new(Method::GET, path)
	}

	pub fn post(path: impl Into<String>) -> Self {
		Self::new(Method::POST, path)
	}

	pub fn put(path: impl Into<String>) -> Self {
		Self::new(Method::PUT, path)
	}

	pub fn patch(path: impl Into<String>) -> Self {
		Self::new(Method::PATCH, path)
	}

	pub fn delete(path: impl Into<String>) -> Self {
		Self::new(Method::DELETE, path)
	}

	pub fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
		self.body = match serde_json::to_vec(body) {
			Ok(bytes) => Body::Json(bytes.into()),
			Err(_) => Body::Invalid,
		};
		self
	}

	pub fn multipart<T: Serialize + ?Sized>(mut self, files: Vec<UploadFile>, payload_json: &T) -> Self {
		self.body = match serde_json::to_string(payload_json) {
			Ok(payload_json) => Body::Multipart { payload_json, files },
			Err(_) => Body::Invalid,
		};
		self
	}

	pub fn multipart_fields(
		mut self,
		fields: Vec<(String, String)>,
		files: Vec<(String, UploadFile)>,
	) -> Self {
		self.body = Body::Form { fields, files };
		self
	}

	pub fn query<K: AsRef<str>, V: AsRef<str>>(mut self, pairs: &[(K, V)]) -> Self {
		self.query.extend(
			pairs
				.iter()
				.map(|(key, value)| (key.as_ref().to_owned(), value.as_ref().to_owned())),
		);
		self
	}

	pub fn audit_reason(mut self, reason: &str) -> Self {
		self.audit_reason = Some(reason.to_owned());
		self
	}

	pub fn idempotent(mut self) -> Self {
		self.idempotent = true;
		self
	}
}

pub fn encode_component(value: &str) -> String {
	let mut encoded = String::with_capacity(value.len());
	for byte in value.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
			encoded.push(byte as char);
		} else {
			encoded.push_str(&format!("%{byte:02X}"));
		}
	}
	encoded
}
