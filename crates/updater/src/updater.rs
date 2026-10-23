use std::path::PathBuf;

use semver::Version;
use serde::Deserialize;
use url::Url;

use crate::download::{Staged, download};
use crate::error::{Result, UpdaterError};
use crate::install::{InstallOutcome, install};
use crate::keys::UPDATE_PUBLIC_KEY;
use crate::manifest::{
	Artifact, MANIFEST_ASSET, MAX_MANIFEST_BYTES, MAX_SIGNATURE_BYTES, ReleaseManifest, SIGNATURE_ASSET,
};
use crate::net::Net;
use crate::target::{Channel, InstallEnv, Target};
use crate::verify::verify_manifest;

const MAX_API_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct UpdaterConfig {
	pub repo: String,
	pub current_version: Version,
	pub target: Target,
	pub channel: Channel,
	pub data_dir: PathBuf,
	pub allow_prerelease: bool,
}

impl UpdaterConfig {
	pub fn new(repo: impl Into<String>, data_dir: PathBuf) -> Result<Self> {
		Ok(Self {
			repo: repo.into(),
			current_version: Version::parse(env!("CARGO_PKG_VERSION"))
				.map_err(|_| UpdaterError::Invalid("package version is not semver".to_owned()))?,
			target: Target::current().ok_or(UpdaterError::UnsupportedTarget)?,
			channel: Channel::Stable,
			data_dir,
			allow_prerelease: false,
		})
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
	pub version: Version,
	pub notes_url: String,
	pub artifact: Option<Artifact>,
}

#[derive(Debug, Deserialize)]
struct Release {
	tag_name: String,
	#[serde(default)]
	draft: bool,
	#[serde(default)]
	prerelease: bool,
	#[serde(default)]
	assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
	name: String,
	browser_download_url: String,
}

impl Release {
	fn version(&self) -> Option<Version> {
		Version::parse(self.tag_name.trim_start_matches('v')).ok()
	}

	fn asset_url(&self, name: &'static str) -> Result<&str> {
		self.assets
			.iter()
			.find(|asset| asset.name == name)
			.map(|asset| asset.browser_download_url.as_str())
			.ok_or(UpdaterError::MissingAsset(name))
	}
}

pub struct Updater {
	config: UpdaterConfig,
	public_key: [u8; 32],
	env: InstallEnv,
	net: Net,
}

impl Updater {
	pub fn new(config: UpdaterConfig) -> Result<Self> {
		Self::with_public_key(config, UPDATE_PUBLIC_KEY)
	}

	pub fn with_public_key(config: UpdaterConfig, public_key: [u8; 32]) -> Result<Self> {
		Ok(Self {
			config,
			public_key,
			env: InstallEnv::detect()?,
			net: Net::github()?,
		})
	}

	pub fn with_api_base(mut self, base: Url) -> Result<Self> {
		self.net = Net::local(base)?;
		Ok(self)
	}

	pub fn with_install_env(mut self, env: InstallEnv) -> Self {
		self.env = env;
		self
	}

	pub fn install_env(&self) -> &InstallEnv {
		&self.env
	}

	pub async fn check(&self) -> Result<Option<AvailableUpdate>> {
		let Some(release) = self.newest_release().await? else {
			return Ok(None);
		};
		let manifest_url = self.net.parse_url(release.asset_url(MANIFEST_ASSET)?)?;
		let signature_url = self.net.parse_url(release.asset_url(SIGNATURE_ASSET)?)?;
		let manifest_bytes = self.net.get_capped(&manifest_url, MAX_MANIFEST_BYTES).await?;
		let signature = self.net.get_capped(&signature_url, MAX_SIGNATURE_BYTES).await?;
		verify_manifest(&self.public_key, &manifest_bytes, &signature)?;
		let manifest = ReleaseManifest::parse(&manifest_bytes)?;
		let version = manifest.semver()?;
		if version <= self.config.current_version || !self.accepts(&version) {
			return Ok(None);
		}
		let artifact = self.env.kind.and_then(|kind| {
			manifest
				.artifact_for(self.config.target, kind.artifact_kind())
				.cloned()
		});
		tracing::info!(%version, installable = artifact.is_some(), "update available");
		Ok(Some(AvailableUpdate {
			version,
			notes_url: manifest.notes_url,
			artifact,
		}))
	}

	pub async fn download(&self, update: &AvailableUpdate, progress: impl Fn(u64, u64)) -> Result<Staged> {
		let artifact = update.artifact.as_ref().ok_or(UpdaterError::CheckOnly)?;
		download(
			&self.net,
			&self.config.data_dir,
			&update.version,
			artifact,
			progress,
		)
		.await
	}

	pub fn install(&self, staged: &Staged) -> Result<InstallOutcome> {
		install(&self.env, &self.config.data_dir, staged)
	}

	fn includes_prerelease(&self) -> bool {
		self.config.allow_prerelease || self.config.channel == Channel::Beta
	}

	fn accepts(&self, version: &Version) -> bool {
		version.pre.is_empty() || self.includes_prerelease()
	}

	async fn newest_release(&self) -> Result<Option<Release>> {
		let repo = &self.config.repo;
		let releases = if self.includes_prerelease() {
			let url = self.net.api_url(&format!("repos/{repo}/releases?per_page=20"))?;
			match self.net.get_capped(&url, MAX_API_BYTES).await {
				Ok(body) => serde_json::from_slice::<Vec<serde_json::Value>>(&body)?
					.into_iter()
					.filter_map(|value| serde_json::from_value::<Release>(value).ok())
					.collect(),
				Err(UpdaterError::Status(404)) => Vec::new(),
				Err(error) => return Err(error),
			}
		} else {
			let url = self.net.api_url(&format!("repos/{repo}/releases/latest"))?;
			match self.net.get_capped(&url, MAX_API_BYTES).await {
				Ok(body) => vec![serde_json::from_slice::<Release>(&body)?],
				Err(UpdaterError::Status(404)) => Vec::new(),
				Err(error) => return Err(error),
			}
		};
		Ok(releases
			.into_iter()
			.filter(|release| !release.draft && (!release.prerelease || self.includes_prerelease()))
			.filter_map(|release| release.version().map(|version| (version, release)))
			.filter(|(version, _)| *version > self.config.current_version)
			.max_by(|(left, _), (right, _)| left.cmp(right))
			.map(|(_, release)| release))
	}
}
