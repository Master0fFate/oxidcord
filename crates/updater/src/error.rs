use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum UpdaterError {
	#[error("network error: {0}")]
	Network(reqwest::Error),
	#[error("server answered with status {0}")]
	Status(u16),
	#[error("response is larger than {limit} bytes")]
	TooLarge { limit: u64 },
	#[error("url is not allowed")]
	UnsafeUrl,
	#[error("release has no {0} asset")]
	MissingAsset(&'static str),
	#[error("invalid release data: {0}")]
	Invalid(String),
	#[error("manifest signature is invalid")]
	BadSignature,
	#[error("sha256 mismatch")]
	HashMismatch,
	#[error("length mismatch")]
	LengthMismatch,
	#[error("this install is managed by a package manager")]
	CheckOnly,
	#[error("this platform has no release target")]
	UnsupportedTarget,
	#[error("archive entry `{0}` is not safe")]
	UnsafeArchive(String),
	#[error("install failed: {0}")]
	Install(String),
	#[error("io error at {path}: {source}")]
	Io { path: PathBuf, source: std::io::Error },
	#[error("zip error: {0}")]
	Zip(#[from] zip::result::ZipError),
	#[error("json error: {0}")]
	Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, UpdaterError>;

impl UpdaterError {
	pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
		Self::Io {
			path: path.into(),
			source,
		}
	}
}

impl From<reqwest::Error> for UpdaterError {
	fn from(error: reqwest::Error) -> Self {
		Self::Network(error.without_url())
	}
}
