use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::VerifyingKey;
use plugin_host::MAX_PACKAGE_BYTES;
use plugin_host::catalog::{Catalog, CatalogEntry, verify_catalog};
use reqwest::{Client, Response, redirect};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub(crate) const CATALOG_PUBLIC_KEY: &str = "GA7yuW3UjVN/kRI3EPirIoYzx9kGIhdLjagsxpcwPJY=";

const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: usize = 1024;
const MAX_REDIRECTS: usize = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const SIGNATURE_SUFFIX: &str = ".sig";

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

pub(crate) async fn fetch_catalog(url: &str) -> Result<Catalog> {
	let key = catalog_key().context("the catalog key is not configured")?;
	let client = client()?;
	let bytes = get_limited(&client, url, MAX_CATALOG_BYTES).await?;
	let signature = get_limited(&client, &format!("{url}{SIGNATURE_SUFFIX}"), MAX_SIGNATURE_BYTES).await?;
	Ok(verify_catalog(&bytes, &signature, &key)?)
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
