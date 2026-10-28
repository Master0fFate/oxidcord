use std::path::{Component, Path};

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::hook::Hook;
use crate::permission::Permission;

pub const API_VERSION: Version = Version::new(1, 2, 0);

const DISCORD_DOMAINS: [&str; 5] = [
	"discord.com",
	"discordapp.com",
	"discord.gg",
	"discord.media",
	"discordapp.net",
];

pub const URL_SCHEMES: [&str; 5] = ["spotify", "steam", "tidal", "itms-apps", "music"];

pub const MANIFEST_FILE: &str = "plugin.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawPermissionRequest")]
pub struct PermissionRequest {
	pub permission: Permission,
	pub reason: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawPermissionRequest {
	Bare(Permission),
	Full {
		permission: Permission,
		#[serde(default)]
		reason: String,
	},
}

impl From<RawPermissionRequest> for PermissionRequest {
	fn from(raw: RawPermissionRequest) -> Self {
		match raw {
			RawPermissionRequest::Bare(permission) => Self {
				permission,
				reason: String::new(),
			},
			RawPermissionRequest::Full { permission, reason } => Self { permission, reason },
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
	pub id: String,
	pub name: String,
	pub version: Version,
	pub api: VersionReq,
	#[serde(default)]
	pub authors: Vec<String>,
	#[serde(default)]
	pub license: String,
	#[serde(default)]
	pub description: String,
	#[serde(default)]
	pub permissions: Vec<PermissionRequest>,
	#[serde(default)]
	pub network_hosts: Vec<String>,
	#[serde(default)]
	pub image_hosts: Vec<String>,
	#[serde(default)]
	pub url_schemes: Vec<String>,
	#[serde(default)]
	pub hooks: Vec<Hook>,
	#[serde(default)]
	pub entry: Option<String>,
	#[serde(default)]
	pub min_host: Option<Version>,
	#[serde(default)]
	pub priority: i32,
	#[serde(default)]
	pub settings_schema: Option<serde_json::Value>,
}

impl Manifest {
	pub fn parse(source: &str) -> Result<Self> {
		let manifest: Self = toml::from_str(source).map_err(|error| Error::Manifest(error.to_string()))?;
		manifest.validate()?;
		Ok(manifest)
	}

	pub fn validate(&self) -> Result<()> {
		validate_id(&self.id)?;
		if self.name.trim().is_empty() {
			return Err(Error::Manifest("name is empty".to_owned()));
		}
		if let Some(entry) = &self.entry {
			validate_entry(entry)?;
		}
		for host in self.network_hosts.iter().chain(&self.image_hosts) {
			validate_host(host)?;
		}
		if let Some(host) = self.network_hosts.iter().find(|host| is_discord_host(host)) {
			return Err(Error::Manifest(format!(
				"network host `{host}` belongs to Discord and cannot be fetched by plugins"
			)));
		}
		if !self.network_hosts.is_empty() && !self.requests(Permission::NetFetch) {
			return Err(Error::Manifest(
				"network_hosts requires the net-fetch permission".to_owned(),
			));
		}
		if let Some(scheme) = self
			.url_schemes
			.iter()
			.find(|scheme| !URL_SCHEMES.contains(&scheme.as_str()))
		{
			return Err(Error::Manifest(format!(
				"url scheme `{scheme}` is not one of {}",
				URL_SCHEMES.join(", ")
			)));
		}
		if !self.url_schemes.is_empty() && !self.requests(Permission::ActionsOpenUrl) {
			return Err(Error::Manifest(
				"url_schemes requires the actions-open-url permission".to_owned(),
			));
		}
		Ok(())
	}

	pub fn requests(&self, permission: Permission) -> bool {
		self.permissions
			.iter()
			.any(|request| request.permission == permission)
	}

	pub fn supports_api(&self) -> bool {
		self.api.matches(&API_VERSION)
	}
}

fn validate_id(id: &str) -> Result<()> {
	let valid_chars = id
		.bytes()
		.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-');
	let valid_shape = id.len() >= 3
		&& id.len() <= 128
		&& id.contains('.')
		&& !id.starts_with(['.', '-'])
		&& !id.ends_with(['.', '-'])
		&& !id.contains("..");
	if valid_chars && valid_shape {
		Ok(())
	} else {
		Err(Error::Manifest(format!(
			"id `{id}` must be a reverse-dns name of 3 to 128 characters in [a-z0-9.-]"
		)))
	}
}

fn validate_entry(entry: &str) -> Result<()> {
	if !entry.ends_with(".wasm") || !is_safe_relative(Path::new(entry)) {
		return Err(Error::Manifest(format!(
			"entry `{entry}` must be a relative path to a .wasm file inside the package"
		)));
	}
	Ok(())
}

pub(crate) fn is_discord_host(host: &str) -> bool {
	DISCORD_DOMAINS.iter().any(|domain| {
		host == *domain
			|| host
				.strip_suffix(domain)
				.is_some_and(|prefix| prefix.ends_with('.'))
	})
}

fn validate_host(host: &str) -> Result<()> {
	let valid = !host.is_empty()
		&& host.len() <= 253
		&& host.contains('.')
		&& host
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-')
		&& !host.starts_with(['.', '-'])
		&& !host.ends_with(['.', '-']);
	if valid {
		Ok(())
	} else {
		Err(Error::Manifest(format!(
			"network host `{host}` is not a lowercase hostname"
		)))
	}
}

pub(crate) fn is_safe_relative(path: &Path) -> bool {
	!path.as_os_str().is_empty()
		&& path
			.components()
			.all(|component| matches!(component, Component::Normal(_)))
}
