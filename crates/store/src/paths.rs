use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct Paths {
	data: PathBuf,
	config: PathBuf,
	cache: PathBuf,
}

impl Paths {
	pub fn discover() -> Result<Self> {
		let dirs = ProjectDirs::from("dev", "oxidcord", "Oxidcord").ok_or_else(|| {
			io::Error::new(
				io::ErrorKind::NotFound,
				"no home directory to place application data in",
			)
		})?;
		Self::create(
			dirs.data_dir().to_path_buf(),
			dirs.config_dir().to_path_buf(),
			dirs.cache_dir().to_path_buf(),
		)
	}

	pub fn at(root: impl AsRef<Path>) -> Result<Self> {
		let root = root.as_ref();
		Self::create(root.join("data"), root.join("config"), root.join("cache"))
	}

	fn create(data: PathBuf, config: PathBuf, cache: PathBuf) -> Result<Self> {
		let paths = Self { data, config, cache };
		for dir in [
			&paths.data,
			&paths.config,
			&paths.cache,
			&paths.plugins_dir(),
			&paths.themes_dir(),
			&paths.logs_dir(),
		] {
			fs::create_dir_all(dir)?;
		}
		Ok(paths)
	}

	pub fn data_dir(&self) -> &Path {
		&self.data
	}

	pub fn config_dir(&self) -> &Path {
		&self.config
	}

	pub fn cache_dir(&self) -> &Path {
		&self.cache
	}

	pub fn database_path(&self) -> PathBuf {
		self.data.join("oxidcord.db")
	}

	pub fn settings_path(&self) -> PathBuf {
		self.config.join("settings.json")
	}

	pub fn token_path(&self) -> PathBuf {
		self.config.join("token")
	}

	pub fn cache_key_path(&self) -> PathBuf {
		self.config.join("cache-key")
	}

	pub fn plugins_dir(&self) -> PathBuf {
		self.data.join("plugins")
	}

	pub fn themes_dir(&self) -> PathBuf {
		self.data.join("themes")
	}

	pub fn icons_dir(&self) -> PathBuf {
		self.data.join("icons")
	}

	pub fn logs_dir(&self) -> PathBuf {
		self.data.join("logs")
	}
}
