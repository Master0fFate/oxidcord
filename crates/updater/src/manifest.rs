use semver::Version;
use serde::{Deserialize, Serialize};

use crate::error::{Result, UpdaterError};
use crate::target::{ArtifactKind, Target};

pub const MANIFEST_ASSET: &str = "manifest.json";
pub const SIGNATURE_ASSET: &str = "manifest.json.sig";
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub const MAX_SIGNATURE_BYTES: u64 = 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
	pub target: Target,
	pub kind: ArtifactKind,
	pub url: String,
	pub sha256: String,
	pub length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
	pub version: String,
	pub published_at: String,
	pub notes_url: String,
	pub artifacts: Vec<Artifact>,
}

impl ReleaseManifest {
	pub fn parse(bytes: &[u8]) -> Result<Self> {
		Ok(serde_json::from_slice(bytes)?)
	}

	pub fn semver(&self) -> Result<Version> {
		Version::parse(&self.version)
			.map_err(|_| UpdaterError::Invalid(format!("version `{}` is not semver", self.version)))
	}

	pub fn artifact_for(&self, target: Target, kind: ArtifactKind) -> Option<&Artifact> {
		self.artifacts
			.iter()
			.find(|artifact| artifact.target == target && artifact.kind == kind)
	}
}

impl Artifact {
	pub fn validate(&self) -> Result<()> {
		let digest_ok = self.sha256.len() == 64 && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
		if !digest_ok {
			return Err(UpdaterError::Invalid(
				"artifact sha256 is not 64 hex digits".to_owned(),
			));
		}
		if self.length == 0 || self.length > MAX_ARTIFACT_BYTES {
			return Err(UpdaterError::Invalid(
				"artifact length is out of range".to_owned(),
			));
		}
		Ok(())
	}
}
