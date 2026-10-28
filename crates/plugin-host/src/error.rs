use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("io error at {path}: {source}")]
	Io { path: PathBuf, source: std::io::Error },
	#[error("invalid manifest: {0}")]
	Manifest(String),
	#[error("invalid package: {0}")]
	Package(String),
	#[error("invalid catalog: {0}")]
	Catalog(String),
	#[error("zip error: {0}")]
	Zip(#[from] zip::result::ZipError),
	#[error("store error: {0}")]
	Store(#[from] store::StoreError),
	#[error("json error: {0}")]
	Json(#[from] serde_json::Error),
	#[error("wasm error: {0}")]
	Wasm(String),
	#[error("unknown plugin {0}")]
	UnknownPlugin(String),
	#[error("plugin {0} is not running")]
	NotRunning(String),
	#[error("incompatible plugin: {0}")]
	Incompatible(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
	pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
		Self::Io {
			path: path.into(),
			source,
		}
	}

	pub fn wasm(error: impl std::fmt::Display) -> Self {
		Self::Wasm(format!("{error:#}"))
	}
}
