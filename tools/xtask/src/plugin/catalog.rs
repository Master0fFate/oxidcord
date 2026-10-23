use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use clap::Args;
use ed25519_dalek::{Signer, SigningKey};
use plugin_host::Package;
use plugin_host::catalog::{CATALOG_VERSION, Catalog, CatalogEntry, verify_catalog};
use sha2::{Digest, Sha256};

use super::{pack_plugin, plugin_dirs, read_manifest};

const FIXTURE_PLUGIN_ID: &str = "dev.oxidcord.conformance-probe";

#[derive(Args)]
pub struct CatalogArgs {
	#[arg(long, default_value = "plugins")]
	dir: PathBuf,
	#[arg(long, default_value = "plugins/catalog.json")]
	out: PathBuf,
	#[arg(
		long,
		default_value = "dist/plugins",
		help = "Where the packed plugins are written"
	)]
	packed: PathBuf,
	#[arg(long, default_value = "", help = "Prefix for the url of each plugin file")]
	base_url: String,
}

#[derive(Args)]
pub struct KeygenArgs {
	out: PathBuf,
}

#[derive(Args)]
pub struct SignArgs {
	catalog: PathBuf,
	key: PathBuf,
}

pub fn run(args: CatalogArgs) -> Result<()> {
	let mut plugins = Vec::new();
	for dir in plugin_dirs(&args.dir)? {
		if read_manifest(&dir)?.id == FIXTURE_PLUGIN_ID {
			continue;
		}
		let archive = pack_plugin(&dir, &args.packed, None)?;
		plugins.push(entry_for(&archive, &args.base_url)?);
	}
	plugins.sort_by(|left, right| left.id.cmp(&right.id));
	let catalog = Catalog {
		version: CATALOG_VERSION,
		generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
		plugins,
	};
	let json = serde_json::to_string_pretty(&catalog)? + "\n";
	fs::write(&args.out, json).with_context(|| format!("could not write {}", args.out.display()))?;
	println!("{} plugins in {}", catalog.plugins.len(), args.out.display());
	Ok(())
}

fn entry_for(archive: &Path, base_url: &str) -> Result<CatalogEntry> {
	let bytes = fs::read(archive).with_context(|| format!("could not read {}", archive.display()))?;
	let manifest = Package::open(archive)?.manifest;
	let file_name = archive.file_name().unwrap_or_default().to_string_lossy();
	let url = if base_url.is_empty() {
		file_name.into_owned()
	} else {
		format!("{}/{file_name}", base_url.trim_end_matches('/'))
	};
	Ok(CatalogEntry {
		id: manifest.id,
		name: manifest.name,
		version: manifest.version,
		description: manifest.description,
		permissions: manifest
			.permissions
			.iter()
			.map(|request| request.permission)
			.collect(),
		url,
		sha256: hex::encode(Sha256::digest(&bytes)),
		length: bytes.len() as u64,
		reviewed: false,
	})
}

pub fn keygen(args: &KeygenArgs) -> Result<()> {
	let key = SigningKey::from_bytes(&rand::random());
	let mut options = fs::OpenOptions::new();
	options.write(true).create_new(true);
	#[cfg(unix)]
	std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
	let mut file = options
		.open(&args.out)
		.with_context(|| format!("could not create {}", args.out.display()))?;
	writeln!(file, "{}", STANDARD.encode(key.to_bytes()))?;
	println!("{}", STANDARD.encode(key.verifying_key().to_bytes()));
	Ok(())
}

pub fn read_signing_key(path: &Path) -> Result<SigningKey> {
	let text = fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
	let bytes = STANDARD
		.decode(text.trim())
		.with_context(|| format!("{} is not base64", path.display()))?;
	let seed = <[u8; 32]>::try_from(bytes.as_slice())
		.map_err(|_| anyhow!("{} must hold a 32 byte key", path.display()))?;
	Ok(SigningKey::from_bytes(&seed))
}

pub fn sign(args: &SignArgs) -> Result<()> {
	let bytes =
		fs::read(&args.catalog).with_context(|| format!("could not read {}", args.catalog.display()))?;
	let key = read_signing_key(&args.key)?;
	let signature = STANDARD.encode(key.sign(&bytes).to_bytes());
	verify_catalog(&bytes, signature.as_bytes(), &key.verifying_key())?;
	let mut signature_path = OsString::from(&args.catalog);
	signature_path.push(".sig");
	let signature_path = PathBuf::from(signature_path);
	fs::write(&signature_path, signature + "\n")
		.with_context(|| format!("could not write {}", signature_path.display()))?;
	println!("{}", signature_path.display());
	Ok(())
}
