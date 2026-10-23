use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::plugin::{SignArgs, sign};

const CATALOG_VERSION: u32 = 1;
const MAX_ID_LENGTH: usize = 64;

#[derive(Args)]
pub struct ThemeArgs {
	#[command(subcommand)]
	command: ThemeCommand,
}

#[derive(Subcommand)]
enum ThemeCommand {
	#[command(about = "Read every theme folder and write a catalog json")]
	Catalog(CatalogArgs),
	#[command(about = "Sign a catalog json with an ed25519 key")]
	Sign(SignArgs),
}

#[derive(Args)]
struct CatalogArgs {
	#[arg(long, default_value = "themes")]
	dir: PathBuf,
	#[arg(long, default_value = "themes/catalog.json")]
	out: PathBuf,
	#[arg(long, help = "Prefix for the url of each theme.json")]
	base_url: String,
}

pub fn run(args: ThemeArgs) -> Result<()> {
	match args.command {
		ThemeCommand::Catalog(args) => catalog(&args),
		ThemeCommand::Sign(args) => sign(&args),
	}
}

fn catalog(args: &CatalogArgs) -> Result<()> {
	let mut themes = Vec::new();
	for entry in fs::read_dir(&args.dir).with_context(|| format!("could not read {}", args.dir.display()))? {
		let folder = entry?.path();
		if folder.join("theme.toml").is_file() {
			themes.push(entry_for(&folder, &args.base_url)?);
		}
	}
	themes.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
	let count = themes.len();
	let catalog = json!({
		"version": CATALOG_VERSION,
		"generated_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
		"themes": themes,
	});
	let text = serde_json::to_string_pretty(&catalog)? + "\n";
	fs::write(&args.out, text).with_context(|| format!("could not write {}", args.out.display()))?;
	println!("{count} themes in {}", args.out.display());
	Ok(())
}

fn entry_for(folder: &std::path::Path, base_url: &str) -> Result<Value> {
	let meta_path = folder.join("theme.toml");
	let meta: toml::Table = fs::read_to_string(&meta_path)
		.with_context(|| format!("could not read {}", meta_path.display()))?
		.parse()
		.with_context(|| format!("invalid {}", meta_path.display()))?;
	let field = |name: &str| -> Result<&str> {
		meta.get(name)
			.and_then(|value| value.as_str())
			.filter(|value| !value.is_empty())
			.with_context(|| format!("{} needs a `{name}` string", meta_path.display()))
	};
	let id = field("id")?;
	if !valid_id(id) {
		bail!(
			"{}: id `{id}` must match [a-z0-9-]{{1,{MAX_ID_LENGTH}}}",
			meta_path.display()
		);
	}
	let theme_path = folder.join("theme.json");
	let bytes = fs::read(&theme_path).with_context(|| format!("could not read {}", theme_path.display()))?;
	if !serde_json::from_slice::<Value>(&bytes).is_ok_and(|value| value.is_object()) {
		bail!("{} must be a JSON object", theme_path.display());
	}
	Ok(json!({
		"id": id,
		"name": field("name")?,
		"version": field("version")?,
		"description": field("description")?,
		"author": field("author")?,
		"url": format!("{}/{id}/theme.json", base_url.trim_end_matches('/')),
		"sha256": hex::encode(Sha256::digest(&bytes)),
		"length": bytes.len(),
	}))
}

fn valid_id(id: &str) -> bool {
	(1..=MAX_ID_LENGTH).contains(&id.len())
		&& id
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
