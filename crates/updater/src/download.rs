use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use semver::Version;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::{Result, UpdaterError};
use crate::manifest::Artifact;
use crate::net::Net;
use crate::target::ArtifactKind;

pub const UPDATES_DIR: &str = "updates";

#[derive(Debug, Clone)]
pub struct Staged {
	pub(crate) version: Version,
	pub(crate) kind: ArtifactKind,
	pub(crate) path: PathBuf,
	pub(crate) sha256: String,
	pub(crate) length: u64,
}

impl Staged {
	pub fn version(&self) -> &Version {
		&self.version
	}

	pub fn kind(&self) -> ArtifactKind {
		self.kind
	}

	pub fn path(&self) -> &Path {
		&self.path
	}

	pub(crate) fn verify(&self) -> Result<()> {
		verify_file(&self.path, &self.sha256, self.length)
	}
}

pub(crate) async fn download(
	net: &Net,
	data_dir: &Path,
	version: &Version,
	artifact: &Artifact,
	progress: impl Fn(u64, u64),
) -> Result<Staged> {
	artifact.validate()?;
	let url = net.parse_url(&artifact.url)?;
	let dir = data_dir.join(UPDATES_DIR).join(version.to_string());
	fs::create_dir_all(&dir)
		.await
		.map_err(|error| UpdaterError::io(&dir, error))?;
	let file_name = format!(
		"oxidcord-{version}-{}.{}",
		artifact.target.as_str(),
		artifact.kind.extension()
	);
	let part = dir.join(format!("{file_name}.part"));
	let path = dir.join(&file_name);
	let response = net.get_file(&url).await?;
	if response
		.content_length()
		.is_some_and(|length| length != artifact.length)
	{
		return Err(UpdaterError::LengthMismatch);
	}
	if let Err(error) = write_verified(response, &part, artifact, &progress).await {
		let _ = fs::remove_file(&part).await;
		return Err(error);
	}
	fs::rename(&part, &path)
		.await
		.map_err(|error| UpdaterError::io(&path, error))?;
	Ok(Staged {
		version: version.clone(),
		kind: artifact.kind,
		path,
		sha256: artifact.sha256.to_ascii_lowercase(),
		length: artifact.length,
	})
}

async fn write_verified(
	response: reqwest::Response,
	part: &Path,
	artifact: &Artifact,
	progress: &impl Fn(u64, u64),
) -> Result<()> {
	let mut file = fs::File::create(part)
		.await
		.map_err(|error| UpdaterError::io(part, error))?;
	let mut hasher = Sha256::new();
	let mut received = 0u64;
	let mut stream = response.bytes_stream();
	while let Some(chunk) = stream.next().await {
		let chunk = chunk?;
		received += chunk.len() as u64;
		if received > artifact.length {
			return Err(UpdaterError::TooLarge {
				limit: artifact.length,
			});
		}
		hasher.update(&chunk);
		file.write_all(&chunk)
			.await
			.map_err(|error| UpdaterError::io(part, error))?;
		progress(received, artifact.length);
	}
	file.sync_all()
		.await
		.map_err(|error| UpdaterError::io(part, error))?;
	if received != artifact.length {
		return Err(UpdaterError::LengthMismatch);
	}
	if !hex::encode(hasher.finalize()).eq_ignore_ascii_case(&artifact.sha256) {
		return Err(UpdaterError::HashMismatch);
	}
	Ok(())
}

pub(crate) fn verify_file(path: &Path, sha256: &str, length: u64) -> Result<()> {
	let mut file = File::open(path).map_err(|error| UpdaterError::io(path, error))?;
	let mut hasher = Sha256::new();
	let mut buffer = vec![0u8; 64 * 1024];
	let mut total = 0u64;
	loop {
		let read = file
			.read(&mut buffer)
			.map_err(|error| UpdaterError::io(path, error))?;
		if read == 0 {
			break;
		}
		total += read as u64;
		hasher.update(&buffer[..read]);
	}
	if total != length {
		return Err(UpdaterError::LengthMismatch);
	}
	if !hex::encode(hasher.finalize()).eq_ignore_ascii_case(sha256) {
		return Err(UpdaterError::HashMismatch);
	}
	Ok(())
}
