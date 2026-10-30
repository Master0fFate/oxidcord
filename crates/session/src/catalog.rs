use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::VerifyingKey;
use plugin_host::MAX_PACKAGE_BYTES;
use plugin_host::catalog::{
	Catalog, CatalogEntry, ThemeCatalog, ThemeEntry, is_valid_theme_id, verify_catalog, verify_theme_catalog,
};
use reqwest::{Client, Response, redirect};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub(crate) const CATALOG_PUBLIC_KEY: &str = "GA7yuW3UjVN/kRI3EPirIoYzx9kGIhdLjagsxpcwPJY=";

const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_THEME_CATALOG_BYTES: usize = 1024 * 1024;
const MAX_THEME_BYTES: u64 = 256 * 1024;
const MAX_SIGNATURE_BYTES: usize = 1024;
const MAX_REDIRECTS: usize = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const SIGNATURE_SUFFIX: &str = ".sig";
const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;
const MAX_FONT_FILES: usize = 8;
const MAX_FONT_NAME_LEN: usize = 96;
const FONTS_FOLDER: &str = "fonts";

pub(crate) fn catalog_key() -> Option<VerifyingKey> {
	let bytes: [u8; 32] = STANDARD.decode(CATALOG_PUBLIC_KEY).ok()?.try_into().ok()?;
	VerifyingKey::from_bytes(&bytes).ok()
}

fn client() -> Result<Client> {
	Client::builder()
		.https_only(true)
		.redirect(redirect::Policy::limited(MAX_REDIRECTS))
		.timeout(REQUEST_TIMEOUT)
		.user_agent(protocol::gateway::web_user_agent())
		.build()
		.context("could not build the http client")
}

async fn get(client: &Client, url: &str) -> Result<Response> {
	let response = client.get(url).send().await.context("request failed")?;
	ensure!(
		response.status().is_success(),
		"server answered {}",
		response.status()
	);
	Ok(response)
}

async fn get_limited(client: &Client, url: &str, limit: usize) -> Result<Vec<u8>> {
	let mut response = get(client, url).await?;
	let mut body = Vec::new();
	while let Some(chunk) = response.chunk().await.context("download interrupted")? {
		ensure!(
			body.len() + chunk.len() <= limit,
			"response is larger than {limit} bytes"
		);
		body.extend_from_slice(&chunk);
	}
	Ok(body)
}

async fn fetch_signed(url: &str, limit: usize) -> Result<(Vec<u8>, Vec<u8>)> {
	let client = client()?;
	let bytes = get_limited(&client, url, limit).await?;
	let signature = get_limited(&client, &format!("{url}{SIGNATURE_SUFFIX}"), MAX_SIGNATURE_BYTES).await?;
	Ok((bytes, signature))
}

pub(crate) async fn fetch_catalog(url: &str) -> Result<Catalog> {
	let key = catalog_key().context("the catalog key is not configured")?;
	let (bytes, signature) = fetch_signed(url, MAX_CATALOG_BYTES).await?;
	Ok(verify_catalog(&bytes, &signature, &key)?)
}

pub(crate) async fn fetch_theme_catalog(url: &str) -> Result<ThemeCatalog> {
	let key = catalog_key().context("the catalog key is not configured")?;
	let (bytes, signature) = fetch_signed(url, MAX_THEME_CATALOG_BYTES).await?;
	Ok(verify_theme_catalog(&bytes, &signature, &key)?)
}

pub(crate) async fn download_theme(entry: &ThemeEntry) -> Result<Vec<u8>> {
	ensure!(
		entry.length > 0 && entry.length <= MAX_THEME_BYTES,
		"catalog length for {} is out of range",
		entry.id
	);
	let client = client()?;
	let bytes = get_limited(&client, &entry.url, MAX_THEME_BYTES as usize).await?;
	check_theme(entry, &bytes)?;
	Ok(bytes)
}

pub(crate) fn check_theme(entry: &ThemeEntry, bytes: &[u8]) -> Result<()> {
	ensure!(
		bytes.len() as u64 == entry.length,
		"download is {} bytes, the catalog says {}",
		bytes.len(),
		entry.length
	);
	ensure!(
		hex::encode(Sha256::digest(bytes)).eq_ignore_ascii_case(&entry.sha256),
		"download does not match the catalog hash"
	);
	let theme: serde_json::Value = serde_json::from_slice(bytes).context("theme is not valid json")?;
	ensure!(theme.is_object(), "theme is not a json object");
	Ok(())
}

pub(crate) fn write_theme(themes_dir: &Path, entry: &ThemeEntry, bytes: &[u8]) -> Result<()> {
	ensure!(
		is_valid_theme_id(&entry.id),
		"theme id {:?} is not allowed",
		entry.id
	);
	std::fs::create_dir_all(themes_dir).context("could not create the themes folder")?;
	store::write_atomic(&themes_dir.join(format!("{}.json", entry.id)), bytes, false)
		.context("could not write the theme")
}

pub(crate) async fn download_plugin(entry: &CatalogEntry, destination: &Path) -> Result<()> {
	ensure!(
		entry.length > 0 && entry.length <= MAX_PACKAGE_BYTES,
		"catalog length for {} is out of range",
		entry.id
	);
	let client = client()?;
	let mut response = get(&client, &entry.url).await?;
	let mut file = tokio::fs::File::create(destination)
		.await
		.context("could not create the download file")?;
	let mut hasher = Sha256::new();
	let mut received: u64 = 0;
	while let Some(chunk) = response.chunk().await.context("download interrupted")? {
		received += chunk.len() as u64;
		ensure!(
			received <= entry.length,
			"download is larger than the catalog says"
		);
		hasher.update(&chunk);
		file.write_all(&chunk)
			.await
			.context("could not write the download")?;
	}
	file.flush().await.context("could not write the download")?;
	if received != entry.length {
		bail!("download is {received} bytes, the catalog says {}", entry.length);
	}
	ensure!(
		hex::encode(hasher.finalize()).eq_ignore_ascii_case(&entry.sha256),
		"download does not match the catalog hash"
	);
	Ok(())
}

#[derive(Deserialize)]
struct FontFile {
	file: String,
	url: String,
	sha256: String,
}

fn is_valid_font_file(name: &str) -> bool {
	let extension = Path::new(name)
		.extension()
		.and_then(|extension| extension.to_str());
	name.len() <= MAX_FONT_NAME_LEN
		&& matches!(extension, Some("ttf" | "otf"))
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
		&& !name.starts_with('.')
}

fn theme_font_files(theme: &[u8]) -> Vec<FontFile> {
	let Ok(serde_json::Value::Object(mut tokens)) = serde_json::from_slice(theme) else {
		return Vec::new();
	};
	let Some(serde_json::Value::Array(items)) = tokens.remove("font_files") else {
		return Vec::new();
	};
	items
		.into_iter()
		.filter_map(|item| serde_json::from_value::<FontFile>(item).ok())
		.filter(|font| is_valid_font_file(&font.file))
		.take(MAX_FONT_FILES)
		.collect()
}

pub(crate) async fn download_theme_fonts(theme: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
	let fonts = theme_font_files(theme);
	if fonts.is_empty() {
		return Ok(Vec::new());
	}
	let client = client()?;
	let mut downloaded = Vec::with_capacity(fonts.len());
	for font in fonts {
		let bytes = get_limited(&client, &font.url, MAX_FONT_BYTES).await?;
		ensure!(
			hex::encode(Sha256::digest(&bytes)).eq_ignore_ascii_case(&font.sha256),
			"font {} does not match its hash",
			font.file
		);
		downloaded.push((font.file, bytes));
	}
	Ok(downloaded)
}

pub(crate) fn write_theme_fonts(themes_dir: &Path, fonts: &[(String, Vec<u8>)]) -> Result<()> {
	if fonts.is_empty() {
		return Ok(());
	}
	let folder = themes_dir.join(FONTS_FOLDER);
	std::fs::create_dir_all(&folder).context("could not create the fonts folder")?;
	for (file, bytes) in fonts {
		store::write_atomic(&folder.join(file), bytes, false).context("could not write the font")?;
	}
	Ok(())
}
