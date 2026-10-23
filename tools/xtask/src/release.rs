use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use clap::Subcommand;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use updater::manifest::{Artifact, ReleaseManifest};
use updater::{ArtifactKind, Target};

const REPOSITORY_ENV: &str = "GITHUB_REPOSITORY";
const SIGNING_KEY_ENV: &str = "UPDATE_SIGNING_KEY";

#[derive(clap::Args)]
pub struct ReleaseArgs {
	#[command(subcommand)]
	command: ReleaseCommand,
}

#[derive(Subcommand)]
enum ReleaseCommand {
	Keygen(KeygenArgs),
	Manifest(ManifestArgs),
	SignManifest(SignArgs),
	Notes(NotesArgs),
}

#[derive(clap::Args)]
struct KeygenArgs {
	out: PathBuf,
}

#[derive(clap::Args)]
struct ManifestArgs {
	#[arg(long)]
	version: String,
	#[arg(long)]
	artifacts_dir: PathBuf,
	#[arg(long)]
	out: PathBuf,
	#[arg(long)]
	repo: Option<String>,
	#[arg(long)]
	notes_url: Option<String>,
}

#[derive(clap::Args)]
struct SignArgs {
	manifest: PathBuf,
	#[arg(long, conflicts_with = "key_env")]
	key_file: Option<PathBuf>,
	#[arg(long, default_value = SIGNING_KEY_ENV)]
	key_env: String,
}

#[derive(clap::Args)]
struct NotesArgs {
	#[arg(long)]
	since: Option<String>,
	#[arg(long)]
	out: Option<PathBuf>,
}

pub fn run(args: ReleaseArgs) -> Result<()> {
	match args.command {
		ReleaseCommand::Keygen(args) => keygen(&args.out),
		ReleaseCommand::Manifest(args) => manifest(args),
		ReleaseCommand::SignManifest(args) => sign_manifest(args),
		ReleaseCommand::Notes(args) => notes(args),
	}
}

fn keygen(out: &Path) -> Result<()> {
	let key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
	let mut options = OpenOptions::new();
	options.write(true).create_new(true);
	#[cfg(unix)]
	std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
	let mut file = options
		.open(out)
		.with_context(|| format!("create {}", out.display()))?;
	writeln!(file, "{}", STANDARD.encode(key.to_bytes()))?;
	let bytes = key
		.verifying_key()
		.to_bytes()
		.iter()
		.map(|byte| format!("0x{byte:02x}"))
		.collect::<Vec<_>>()
		.join(", ");
	println!("secret key written to {}", out.display());
	println!("store its contents as the UPDATE_SIGNING_KEY repository secret, then delete the file");
	println!("public key for crates/updater/src/keys.rs:");
	println!("pub const UPDATE_PUBLIC_KEY: [u8; 32] = [{bytes}];");
	Ok(())
}

fn manifest(args: ManifestArgs) -> Result<()> {
	let repo = match args.repo {
		Some(repo) => repo,
		None => std::env::var(REPOSITORY_ENV).context("pass --repo or set GITHUB_REPOSITORY")?,
	};
	let version = args.version.trim_start_matches('v').to_owned();
	let prefix = format!("oxidcord-{version}-");
	let mut artifacts = Vec::new();
	let mut names = fs::read_dir(&args.artifacts_dir)
		.with_context(|| format!("read {}", args.artifacts_dir.display()))?
		.filter_map(|entry| entry.ok()?.file_name().into_string().ok())
		.collect::<Vec<_>>();
	names.sort();
	for name in names {
		let Some((target, kind)) = classify(&name, &prefix) else {
			eprintln!("skipping {name}");
			continue;
		};
		let (sha256, length) = hash_file(&args.artifacts_dir.join(&name))?;
		artifacts.push(Artifact {
			target,
			kind,
			url: format!("https://github.com/{repo}/releases/download/v{version}/{name}"),
			sha256,
			length,
		});
	}
	if artifacts.is_empty() {
		bail!(
			"no artifacts named {prefix}<target>-<kind>.<ext> in {}",
			args.artifacts_dir.display()
		);
	}
	let manifest = ReleaseManifest {
		notes_url: args
			.notes_url
			.unwrap_or_else(|| format!("https://github.com/{repo}/releases/tag/v{version}")),
		published_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
		version,
		artifacts,
	};
	manifest.semver()?;
	let mut json = serde_json::to_vec_pretty(&manifest)?;
	json.push(b'\n');
	fs::write(&args.out, json).with_context(|| format!("write {}", args.out.display()))?;
	println!("{} artifacts in {}", manifest.artifacts.len(), args.out.display());
	Ok(())
}

fn classify(name: &str, prefix: &str) -> Option<(Target, ArtifactKind)> {
	let rest = name.strip_prefix(prefix)?;
	let target = Target::ALL
		.into_iter()
		.find(|target| rest.starts_with(&format!("{}-", target.as_str())))?;
	let tail = &rest[target.as_str().len() + 1..];
	let kind = ArtifactKind::ALL
		.into_iter()
		.find(|kind| tail == format!("{}.{}", kind.as_str(), kind.extension()))?;
	Some((target, kind))
}

fn hash_file(path: &Path) -> Result<(String, u64)> {
	let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
	let mut hasher = Sha256::new();
	let mut buffer = vec![0u8; 64 * 1024];
	let mut length = 0u64;
	loop {
		let read = file.read(&mut buffer)?;
		if read == 0 {
			break;
		}
		hasher.update(&buffer[..read]);
		length += read as u64;
	}
	Ok((hex::encode(hasher.finalize()), length))
}

fn sign_manifest(args: SignArgs) -> Result<()> {
	let encoded = match &args.key_file {
		Some(path) => fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
		None => std::env::var(&args.key_env).with_context(|| format!("{} is not set", args.key_env))?,
	};
	let seed: [u8; 32] = STANDARD
		.decode(encoded.trim())
		.context("secret key is not base64")?
		.try_into()
		.map_err(|_| anyhow::anyhow!("secret key must be 32 bytes"))?;
	let manifest = fs::read(&args.manifest).with_context(|| format!("read {}", args.manifest.display()))?;
	let signature = SigningKey::from_bytes(&seed).sign(&manifest);
	let mut signature_path = args.manifest.clone().into_os_string();
	signature_path.push(".sig");
	let signature_path = PathBuf::from(signature_path);
	fs::write(
		&signature_path,
		format!("{}\n", hex::encode(signature.to_bytes())),
	)
	.with_context(|| format!("write {}", signature_path.display()))?;
	println!("signed {}", args.manifest.display());
	Ok(())
}

fn notes(args: NotesArgs) -> Result<()> {
	let mut command = Command::new("git");
	command.args(["log", "--no-merges", "--pretty=format:%s"]);
	if let Some(tag) = &args.since {
		command.arg(format!("{tag}..HEAD"));
	}
	let output = command.output().context("run git log")?;
	if !output.status.success() {
		bail!(
			"git log failed: {}",
			String::from_utf8_lossy(&output.stderr).trim()
		);
	}
	let mut text = String::from_utf8(output.stdout)?
		.lines()
		.filter(|line| !line.trim().is_empty())
		.map(|line| format!("- {}\n", line.trim()))
		.collect::<String>();
	if text.is_empty() {
		text.push_str("- no changes\n");
	}
	match &args.out {
		Some(path) => fs::write(path, text).with_context(|| format!("write {}", path.display())),
		None => {
			print!("{text}");
			Ok(())
		}
	}
}
