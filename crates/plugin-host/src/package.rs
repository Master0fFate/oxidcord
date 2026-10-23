use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use semver::Version;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{Error, Result};
use crate::manifest::{MANIFEST_FILE, Manifest, is_safe_relative};

pub const PACKAGE_EXTENSION: &str = "oxidcord-plugin";
pub const SIGNATURE_FILE: &str = "signature";
pub const THEME_FILE: &str = "theme.json";
pub const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;

const ASSETS_DIR: &str = "assets";
const PUBLIC_KEY_BYTES: usize = 32;
const SIGNATURE_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
	Unsigned,
	Signed,
	Catalog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFile {
	pub path: PathBuf,
	pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Package {
	pub manifest: Manifest,
	pub manifest_bytes: Vec<u8>,
	pub wasm: Option<Vec<u8>>,
	pub theme: Option<Vec<u8>>,
	pub assets: Vec<PackageFile>,
	pub signature: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct InstalledPlugin {
	pub manifest: Manifest,
	pub dir: PathBuf,
	pub trust: Trust,
}

fn package_error(reason: impl Into<String>) -> Error {
	Error::Package(reason.into())
}

struct Budget(u64);

impl Budget {
	fn take(&mut self, bytes: u64) -> Result<()> {
		self.0 = self
			.0
			.checked_sub(bytes)
			.ok_or_else(|| package_error("package is larger than 64 MB"))?;
		Ok(())
	}

	fn remaining(&self) -> u64 {
		self.0
	}
}

impl Package {
	pub fn open(path: &Path) -> Result<Self> {
		let file = File::open(path).map_err(|error| Error::io(path, error))?;
		let mut archive = ZipArchive::new(BufReader::new(file))?;
		let mut budget = Budget(MAX_PACKAGE_BYTES);
		let mut files: Vec<PackageFile> = Vec::new();
		for index in 0..archive.len() {
			let mut entry = archive.by_index(index)?;
			let name = String::from_utf8_lossy(entry.name_raw()).into_owned();
			if entry.is_symlink() {
				return Err(package_error(format!("symlink entry `{name}`")));
			}
			let relative = entry
				.enclosed_name()
				.filter(|path| is_safe_relative(path))
				.ok_or_else(|| package_error(format!("unsafe path `{name}`")))?;
			if entry.is_dir() {
				continue;
			}
			budget.take(entry.size())?;
			let mut data = Vec::new();
			(&mut entry)
				.take(budget.remaining() + 1)
				.read_to_end(&mut data)
				.map_err(|error| Error::io(path, error))?;
			if data.len() as u64 > entry.size() {
				return Err(package_error(format!("entry `{name}` is larger than declared")));
			}
			files.push(PackageFile { path: relative, data });
		}
		Self::assemble(files)
	}

	fn assemble(files: Vec<PackageFile>) -> Result<Self> {
		let mut manifest_bytes = None;
		let mut signature = None;
		let mut theme = None;
		let mut others = Vec::new();
		for file in files {
			if file.path == Path::new(MANIFEST_FILE) {
				manifest_bytes = Some(file.data);
			} else if file.path == Path::new(SIGNATURE_FILE) {
				signature = Some(file.data);
			} else if file.path == Path::new(THEME_FILE) {
				theme = Some(file.data);
			} else {
				others.push(file);
			}
		}
		let manifest_bytes = manifest_bytes.ok_or_else(|| package_error("missing plugin.toml"))?;
		let manifest_text = std::str::from_utf8(&manifest_bytes)
			.map_err(|_| Error::Manifest("plugin.toml is not utf-8".to_owned()))?;
		let manifest = Manifest::parse(manifest_text)?;
		let wasm = manifest.entry.as_deref().and_then(|entry| {
			others
				.iter()
				.find(|file| file.path == Path::new(entry))
				.map(|file| file.data.clone())
		});
		let assets = others
			.into_iter()
			.filter(|file| file.path.starts_with(ASSETS_DIR))
			.collect();
		Self {
			manifest,
			manifest_bytes,
			wasm,
			theme,
			assets,
			signature,
		}
		.checked()
	}

	fn checked(self) -> Result<Self> {
		match (&self.manifest.entry, &self.wasm) {
			(Some(entry), None) => return Err(package_error(format!("missing entry `{entry}`"))),
			(None, Some(_)) => return Err(package_error("wasm given for a manifest without an entry")),
			_ => {}
		}
		let theme_is_object = self.theme.as_deref().is_none_or(|theme| {
			serde_json::from_slice::<serde_json::Value>(theme).is_ok_and(|value| value.is_object())
		});
		if theme_is_object {
			Ok(self)
		} else {
			Err(package_error("theme.json must be a json object"))
		}
	}

	pub fn from_files(manifest_path: &Path, wasm_path: Option<&Path>) -> Result<Self> {
		let manifest_bytes = fs::read(manifest_path).map_err(|error| Error::io(manifest_path, error))?;
		let wasm = wasm_path
			.map(|path| fs::read(path).map_err(|error| Error::io(path, error)))
			.transpose()?;
		let manifest_text = std::str::from_utf8(&manifest_bytes)
			.map_err(|_| Error::Manifest("plugin.toml is not utf-8".to_owned()))?;
		let manifest = Manifest::parse(manifest_text)?;
		let dir = manifest_path.parent().unwrap_or(Path::new("."));
		let signature = read_optional(&dir.join(SIGNATURE_FILE))?;
		let theme = read_optional(&dir.join(THEME_FILE))?;
		let mut budget = Budget(MAX_PACKAGE_BYTES);
		let fixed_bytes =
			manifest_bytes.len() + wasm.as_ref().map_or(0, Vec::len) + theme.as_ref().map_or(0, Vec::len);
		budget.take(fixed_bytes as u64)?;
		let mut assets = Vec::new();
		collect_assets(
			&dir.join(ASSETS_DIR),
			Path::new(ASSETS_DIR),
			&mut budget,
			&mut assets,
		)?;
		Self {
			manifest,
			manifest_bytes,
			wasm,
			theme,
			assets,
			signature,
		}
		.checked()
	}

	pub fn from_dir(dir: &Path) -> Result<Self> {
		let manifest_path = dir.join(MANIFEST_FILE);
		let manifest_text =
			fs::read_to_string(&manifest_path).map_err(|error| Error::io(&manifest_path, error))?;
		let manifest = Manifest::parse(&manifest_text)?;
		let wasm_path = manifest.entry.as_ref().map(|entry| dir.join(entry));
		Self::from_files(&manifest_path, wasm_path.as_deref())
	}

	pub fn content_hash(&self) -> [u8; 32] {
		let mut hasher = Sha256::new();
		hasher.update(&self.manifest_bytes);
		hasher.update(self.wasm.as_deref().unwrap_or_default());
		hasher.update(self.theme.as_deref().unwrap_or_default());
		hasher.finalize().into()
	}

	pub fn content_hash_hex(&self) -> String {
		hex::encode(self.content_hash())
	}

	pub fn sign(&mut self, key: &SigningKey) {
		let signature = key.sign(&self.content_hash());
		let mut bytes = Vec::with_capacity(PUBLIC_KEY_BYTES + SIGNATURE_BYTES);
		bytes.extend_from_slice(key.verifying_key().as_bytes());
		bytes.extend_from_slice(&signature.to_bytes());
		self.signature = Some(bytes);
	}

	pub fn verify(&self, catalog_key: Option<&VerifyingKey>) -> Trust {
		let Some(bytes) = &self.signature else {
			return Trust::Unsigned;
		};
		let (embedded_key, signature_bytes) = match bytes.len() {
			SIGNATURE_BYTES => (None, bytes.as_slice()),
			len if len == PUBLIC_KEY_BYTES + SIGNATURE_BYTES => {
				let (key, signature) = bytes.split_at(PUBLIC_KEY_BYTES);
				(Some(key), signature)
			}
			_ => return Trust::Unsigned,
		};
		let Ok(signature) = Signature::from_slice(signature_bytes) else {
			return Trust::Unsigned;
		};
		let digest = self.content_hash();
		if catalog_key.is_some_and(|key| key.verify_strict(&digest, &signature).is_ok()) {
			return Trust::Catalog;
		}
		let embedded = embedded_key
			.and_then(|key| <&[u8; PUBLIC_KEY_BYTES]>::try_from(key).ok())
			.and_then(|key| VerifyingKey::from_bytes(key).ok());
		if embedded.is_some_and(|key| key.verify_strict(&digest, &signature).is_ok()) {
			Trust::Signed
		} else {
			Trust::Unsigned
		}
	}

	pub fn install_into(&self, plugins_dir: &Path) -> Result<InstalledPlugin> {
		let id_dir = plugins_dir.join(&self.manifest.id);
		let target = id_dir.join(self.manifest.version.to_string());
		let staging = id_dir.join(format!(".staging-{}", self.manifest.version));
		remove_dir_if_exists(&staging)?;
		self.write_files(&staging)?;
		remove_dir_if_exists(&target)?;
		fs::rename(&staging, &target).map_err(|error| Error::io(&target, error))?;
		Ok(InstalledPlugin {
			manifest: self.manifest.clone(),
			dir: target,
			trust: self.verify(None),
		})
	}

	fn write_files(&self, root: &Path) -> Result<()> {
		write_file(&root.join(MANIFEST_FILE), &self.manifest_bytes)?;
		if let (Some(entry), Some(wasm)) = (&self.manifest.entry, &self.wasm) {
			write_file(&root.join(entry), wasm)?;
		}
		if let Some(theme) = &self.theme {
			write_file(&root.join(THEME_FILE), theme)?;
		}
		for asset in &self.assets {
			write_file(&root.join(&asset.path), &asset.data)?;
		}
		if let Some(signature) = &self.signature {
			write_file(&root.join(SIGNATURE_FILE), signature)?;
		}
		Ok(())
	}

	pub fn pack(&self, destination: &Path) -> Result<()> {
		let file = File::create(destination).map_err(|error| Error::io(destination, error))?;
		let mut writer = ZipWriter::new(file);
		let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
		let mut add = |name: &str, data: &[u8]| -> Result<()> {
			writer.start_file(name, options)?;
			writer
				.write_all(data)
				.map_err(|error| Error::io(destination, error))
		};
		add(MANIFEST_FILE, &self.manifest_bytes)?;
		if let (Some(entry), Some(wasm)) = (&self.manifest.entry, &self.wasm) {
			add(entry, wasm)?;
		}
		if let Some(theme) = &self.theme {
			add(THEME_FILE, theme)?;
		}
		for asset in &self.assets {
			add(&asset.path.to_string_lossy().replace('\\', "/"), &asset.data)?;
		}
		if let Some(signature) = &self.signature {
			add(SIGNATURE_FILE, signature)?;
		}
		writer.finish()?;
		Ok(())
	}
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
	match fs::read(path) {
		Ok(bytes) => Ok(Some(bytes)),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(Error::io(path, error)),
	}
}

fn collect_assets(
	dir: &Path,
	relative: &Path,
	budget: &mut Budget,
	out: &mut Vec<PackageFile>,
) -> Result<()> {
	let entries = match fs::read_dir(dir) {
		Ok(entries) => entries,
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
		Err(error) => return Err(Error::io(dir, error)),
	};
	for entry in entries {
		let entry = entry.map_err(|error| Error::io(dir, error))?;
		let path = entry.path();
		let relative_path = relative.join(entry.file_name());
		let file_type = entry.file_type().map_err(|error| Error::io(&path, error))?;
		if file_type.is_symlink() {
			return Err(package_error(format!("symlink in assets: {}", path.display())));
		}
		if file_type.is_dir() {
			collect_assets(&path, &relative_path, budget, out)?;
		} else {
			let data = fs::read(&path).map_err(|error| Error::io(&path, error))?;
			budget.take(data.len() as u64)?;
			out.push(PackageFile {
				path: relative_path,
				data,
			});
		}
	}
	Ok(())
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
	}
	fs::write(path, data).map_err(|error| Error::io(path, error))
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
	match fs::remove_dir_all(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
		Err(error) => Err(Error::io(path, error)),
	}
}

pub fn discover(plugins_dir: &Path) -> Vec<PathBuf> {
	let Ok(entries) = fs::read_dir(plugins_dir) else {
		return Vec::new();
	};
	let mut found = Vec::new();
	for entry in entries.flatten() {
		let dir = entry.path();
		if !dir.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
			continue;
		}
		if dir.join(MANIFEST_FILE).is_file() {
			found.push(dir);
			continue;
		}
		if let Some(latest) = latest_version_dir(&dir) {
			found.push(latest);
		}
	}
	found.sort();
	found
}

fn latest_version_dir(id_dir: &Path) -> Option<PathBuf> {
	fs::read_dir(id_dir)
		.ok()?
		.flatten()
		.filter_map(|entry| {
			let version = Version::parse(&entry.file_name().to_string_lossy()).ok()?;
			let dir = entry.path();
			dir.join(MANIFEST_FILE).is_file().then_some((version, dir))
		})
		.max_by(|left, right| left.0.cmp(&right.0))
		.map(|(_, dir)| dir)
}
