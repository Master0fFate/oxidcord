use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

const VERSION: &str = "17.0.3";
const SHA256: &str = "a0855654b633045ae2337537e77f1bb4361162f7fcd910e613eaab1d6d9c5fca";
const IMAGE_DIR: &str = "assets/72x72/";
const MAGIC: &[u8; 4] = b"OXTW";

#[derive(clap::Args)]
pub struct TwemojiArgs {
	#[arg(long)]
	out: Option<PathBuf>,
}

pub fn run(args: TwemojiArgs) -> Result<()> {
	let out = args.out.unwrap_or_else(default_output);
	let archive = download()?;
	let mut images = extract(&archive)?;
	images.sort_by(|left, right| left.0.cmp(&right.0));
	let pack = encode(&images)?;
	fs::write(&out, &pack).with_context(|| format!("could not write {}", out.display()))?;
	println!(
		"twemoji {VERSION}: {} images, {} bytes written to {}",
		images.len(),
		pack.len(),
		out.display()
	);
	Ok(())
}

fn default_output() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.ancestors()
		.nth(2)
		.unwrap_or(Path::new("."))
		.join("assets")
		.join("twemoji.pack")
}

fn download() -> Result<Vec<u8>> {
	let url = format!("https://github.com/jdecked/twemoji/archive/refs/tags/v{VERSION}.tar.gz");
	let runtime = tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.context("could not start the async runtime")?;
	let bytes = runtime
		.block_on(async {
			let response = reqwest::get(&url).await?.error_for_status()?;
			response.bytes().await
		})
		.with_context(|| format!("could not download {url}"))?;
	let digest = hex::encode(Sha256::digest(&bytes));
	ensure!(
		digest == SHA256,
		"twemoji checksum mismatch: expected {SHA256}, got {digest}"
	);
	Ok(bytes.to_vec())
}

fn extract(archive: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
	let mut entries = tar::Archive::new(GzDecoder::new(Cursor::new(archive)));
	let mut images = Vec::new();
	for entry in entries.entries()? {
		let mut entry = entry?;
		if !entry.header().entry_type().is_file() {
			continue;
		}
		let path = entry.path()?.to_string_lossy().into_owned();
		let Some(name) = path
			.split_once(IMAGE_DIR)
			.and_then(|(_, name)| name.strip_suffix(".png"))
		else {
			continue;
		};
		let key = name.to_owned();
		let mut bytes = Vec::new();
		entry.read_to_end(&mut bytes)?;
		images.push((key, bytes));
	}
	ensure!(!images.is_empty(), "no images found in the twemoji archive");
	Ok(images)
}

fn encode(images: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
	let mut index = Vec::new();
	let mut blob = Vec::new();
	for (key, bytes) in images {
		let key_length = u8::try_from(key.len()).context("emoji key is too long")?;
		index.push(key_length);
		index.extend_from_slice(key.as_bytes());
		index.extend_from_slice(&u32::try_from(blob.len())?.to_le_bytes());
		index.extend_from_slice(&u32::try_from(bytes.len())?.to_le_bytes());
		blob.extend_from_slice(bytes);
	}
	let mut pack = Vec::with_capacity(8 + index.len() + blob.len());
	pack.extend_from_slice(MAGIC);
	pack.extend_from_slice(&u32::try_from(images.len())?.to_le_bytes());
	pack.extend_from_slice(&index);
	pack.extend_from_slice(&blob);
	Ok(pack)
}
