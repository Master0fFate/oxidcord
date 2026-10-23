mod catalog;
mod scaffold;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use ed25519_dalek::SigningKey;
use plugin_host::conformance::{self, Outcome, Report};
use plugin_host::{MANIFEST_FILE, Manifest, PACKAGE_EXTENSION, Package};

const WASM_TARGET: &str = "wasm32-wasip2";

#[derive(Args)]
pub struct PluginArgs {
	#[command(subcommand)]
	command: PluginCommand,
}

#[derive(Subcommand)]
enum PluginCommand {
	#[command(about = "Scaffold a plugin crate")]
	New(scaffold::NewArgs),
	#[command(about = "Build a plugin for wasm32-wasip2")]
	Build(BuildArgs),
	#[command(about = "Build a plugin and pack it into a .oxidcord-plugin file")]
	Pack(PackArgs),
	#[command(about = "Pack every plugin in a directory and run the conformance suite")]
	Conformance(ConformanceArgs),
	#[command(about = "Pack every plugin in a directory and write a catalog json")]
	Catalog(catalog::CatalogArgs),
	#[command(about = "Write an ed25519 key for signing catalogs")]
	Keygen(catalog::KeygenArgs),
	#[command(about = "Sign a catalog json with an ed25519 key")]
	SignCatalog(catalog::SignArgs),
}

#[derive(Args)]
struct BuildArgs {
	path: PathBuf,
	#[arg(long)]
	release: bool,
}

#[derive(Args)]
struct PackArgs {
	path: PathBuf,
	#[arg(long, default_value = "dist/plugins")]
	out: PathBuf,
	#[arg(long, help = "File with a base64 ed25519 secret key")]
	sign: Option<PathBuf>,
}

#[derive(Args)]
struct ConformanceArgs {
	#[arg(long, default_value = "plugins")]
	dir: PathBuf,
}

pub fn run(args: PluginArgs) -> Result<()> {
	match args.command {
		PluginCommand::New(args) => scaffold::run(args),
		PluginCommand::Build(args) => build(&args),
		PluginCommand::Pack(args) => pack(&args),
		PluginCommand::Conformance(args) => conformance(&args),
		PluginCommand::Catalog(args) => catalog::run(args),
		PluginCommand::Keygen(args) => catalog::keygen(&args),
		PluginCommand::SignCatalog(args) => catalog::sign(&args),
	}
}

fn build(args: &BuildArgs) -> Result<()> {
	match build_wasm(&args.path, args.release)? {
		Some(wasm) => println!("{}", wasm.display()),
		None => println!("{} has no crate, nothing to build", args.path.display()),
	}
	Ok(())
}

fn pack(args: &PackArgs) -> Result<()> {
	let key = args.sign.as_deref().map(catalog::read_signing_key).transpose()?;
	let archive = pack_plugin(&args.path, &args.out, key.as_ref())?;
	println!("{}", archive.display());
	Ok(())
}

pub(crate) fn build_wasm(plugin_dir: &Path, release: bool) -> Result<Option<PathBuf>> {
	let cargo_manifest = plugin_dir.join("Cargo.toml");
	if !cargo_manifest.is_file() {
		return Ok(None);
	}
	let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
	command
		.args([
			"build",
			"--target",
			WASM_TARGET,
			"--message-format=json-render-diagnostics",
		])
		.arg("--manifest-path")
		.arg(&cargo_manifest);
	if release {
		command.arg("--release");
	}
	let output = command
		.stderr(Stdio::inherit())
		.output()
		.context("could not run cargo")?;
	if !output.status.success() {
		bail!("cargo build failed for {}", plugin_dir.display());
	}
	String::from_utf8_lossy(&output.stdout)
		.lines()
		.filter_map(wasm_artifact)
		.next_back()
		.map(Some)
		.with_context(|| format!("cargo produced no .wasm file for {}", plugin_dir.display()))
}

fn wasm_artifact(line: &str) -> Option<PathBuf> {
	let message: serde_json::Value = serde_json::from_str(line).ok()?;
	if message["reason"] != "compiler-artifact" {
		return None;
	}
	message["filenames"]
		.as_array()?
		.iter()
		.filter_map(|name| name.as_str())
		.find(|name| name.ends_with(".wasm"))
		.map(PathBuf::from)
}

fn read_manifest(plugin_dir: &Path) -> Result<Manifest> {
	let path = plugin_dir.join(MANIFEST_FILE);
	let text = fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display()))?;
	Manifest::parse(&text).with_context(|| format!("invalid {}", path.display()))
}

fn pack_plugin(plugin_dir: &Path, out_dir: &Path, key: Option<&SigningKey>) -> Result<PathBuf> {
	let wasm = build_wasm(plugin_dir, true)?;
	let mut package = Package::from_files(&plugin_dir.join(MANIFEST_FILE), wasm.as_deref())?;
	if let Some(key) = key {
		package.sign(key);
	}
	fs::create_dir_all(out_dir).with_context(|| format!("could not create {}", out_dir.display()))?;
	let archive = out_dir.join(format!(
		"{}-{}.{PACKAGE_EXTENSION}",
		package.manifest.id, package.manifest.version
	));
	package.pack(&archive)?;
	Ok(archive)
}

fn plugin_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
	let mut found = Vec::new();
	for entry in fs::read_dir(dir).with_context(|| format!("could not read {}", dir.display()))? {
		let path = entry?.path();
		if path.join(MANIFEST_FILE).is_file() {
			found.push(path);
		}
	}
	found.sort();
	Ok(found)
}

struct Scratch(PathBuf);

impl Scratch {
	fn new() -> Result<Self> {
		let path = std::env::temp_dir().join(format!("oxidcord-xtask-{}", std::process::id()));
		fs::create_dir_all(&path).with_context(|| format!("could not create {}", path.display()))?;
		Ok(Self(path))
	}
}

impl Drop for Scratch {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

fn conformance(args: &ConformanceArgs) -> Result<()> {
	let scratch = Scratch::new()?;
	let packed = scratch.0.join("packed");
	let installed = scratch.0.join("installed");
	for plugin in plugin_dirs(&args.dir)? {
		let archive = pack_plugin(&plugin, &packed, None)?;
		Package::open(&archive)?.install_into(&installed)?;
	}
	let report = conformance::run(&installed);
	print_table(&report);
	if report.passed() {
		Ok(())
	} else {
		bail!("conformance failed")
	}
}

fn print_table(report: &Report) {
	let width = report
		.plugins
		.iter()
		.map(|plugin| plugin.id.len())
		.max()
		.unwrap_or(6)
		.max(6);
	println!(
		"{:<width$}  {:>4}  {:>4}  {:>4}  result",
		"plugin", "pass", "fail", "skip"
	);
	for plugin in &report.plugins {
		let count = |outcome| {
			plugin
				.checks
				.iter()
				.filter(|check| check.outcome == outcome)
				.count()
		};
		let failed = count(Outcome::Fail);
		println!(
			"{:<width$}  {:>4}  {failed:>4}  {:>4}  {}",
			plugin.id,
			count(Outcome::Pass),
			count(Outcome::Skipped),
			if failed == 0 { "ok" } else { "FAILED" }
		);
	}
	for plugin in &report.plugins {
		for check in plugin
			.checks
			.iter()
			.filter(|check| check.outcome == Outcome::Fail)
		{
			println!("{}: {}: {}", plugin.id, check.name, check.detail);
		}
	}
}
