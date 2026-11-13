use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

const RELEASE: &str =
	"https://github.com/FFmpeg-Builds/FFmpeg-Builds/releases/download/autobuild-2026-11-13-12-43";
const BUILD: &str = "ffmpeg-n8.1.3-35-gf1acb0a266";
const LICENSE_FILE: &str = "LICENSE.txt";
const LICENSE_OUT: &str = "LICENSE-FFMPEG.txt";
const WINDOWS_LIBRARIES: [&str; 3] = ["avcodec-62.dll", "avutil-60.dll", "swresample-6.dll"];
const LINUX_LIBRARIES: [&str; 3] = ["libavcodec.so.62", "libavutil.so.60", "libswresample.so.6"];

#[derive(Clone, Copy)]
enum Archive {
	Zip,
	TarXz,
}

struct Pack {
	name: &'static str,
	platform: &'static str,
	sha256: &'static str,
	archive: Archive,
	libraries: [&'static str; 3],
}

const PACKS: [Pack; 4] = [
	Pack {
		name: "windows-x64",
		platform: "win64",
		sha256: "57cb10e4703b88edf55aa49e94dee19fd12f555f82ed3493a8cd70c847699996",
		archive: Archive::Zip,
		libraries: WINDOWS_LIBRARIES,
	},
	Pack {
		name: "windows-arm64",
		platform: "winarm64",
		sha256: "cba8cbe7d9577bd01f6f165a3e59811e481aafe0a12db178c20171eeb5cbb79f",
		archive: Archive::Zip,
		libraries: WINDOWS_LIBRARIES,
	},
	Pack {
		name: "linux-x64",
		platform: "linux64",
		sha256: "67b307fb2b07d2510a8d61cda583695560af1c76e355969e37133e66f98db7c4",
		archive: Archive::TarXz,
		libraries: LINUX_LIBRARIES,
	},
	Pack {
		name: "linux-arm64",
		platform: "linuxarm64",
		sha256: "b3e1af569cb227bee9d2030783a4e92757843d16a04e415e6167183ca4aa3a96",
		archive: Archive::TarXz,
		libraries: LINUX_LIBRARIES,
	},
];

#[derive(clap::Args)]
pub struct CodecArgs {
	#[arg(long)]
	target: Option<String>,
	#[arg(long)]
	out: Vec<PathBuf>,
}

impl Pack {
	fn url(&self) -> String {
		let extension = match self.archive {
			Archive::Zip => "zip",
			Archive::TarXz => "tar.xz",
		};
		format!("{RELEASE}/{BUILD}-{}-lgpl-shared-8.1.{extension}", self.platform)
	}
}

fn host_pack() -> Option<&'static str> {
	match (std::env::consts::OS, std::env::consts::ARCH) {
		("windows", "x86_64") => Some("windows-x64"),
		("windows", "aarch64") => Some("windows-arm64"),
		("linux", "x86_64") => Some("linux-x64"),
		("linux", "aarch64") => Some("linux-arm64"),
		_ => None,
	}
}

fn default_outputs() -> Vec<PathBuf> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.ancestors()
		.nth(2)
		.unwrap_or(Path::new("."));
	let target = root.join("target");
	["debug", "release"]
		.iter()
		.map(|profile| target.join(profile).join("codecs"))
		.collect()
}

pub fn run(args: CodecArgs) -> Result<()> {
	let Some(name) = args.target.clone().or_else(|| host_pack().map(str::to_owned)) else {
		bail!("no bundled codec pack for this platform, install the FFmpeg 8 shared libraries instead");
	};
	let pack = PACKS
		.iter()
		.find(|pack| pack.name == name)
		.with_context(|| format!("unknown codec pack {name}"))?;
	let outputs = if args.out.is_empty() {
		default_outputs()
	} else {
		args.out
	};
	let archive = download(pack)?;
	let files = extract(pack, &archive)?;
	for out in &outputs {
		fs::create_dir_all(out).with_context(|| format!("could not create {}", out.display()))?;
		for (file, bytes) in &files {
			fs::write(out.join(file), bytes).with_context(|| format!("could not write {file}"))?;
		}
		println!("codec pack {} installed into {}", pack.name, out.display());
	}
	Ok(())
}

fn download(pack: &Pack) -> Result<Vec<u8>> {
	let url = pack.url();
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
		digest == pack.sha256,
		"codec pack checksum mismatch: expected {}, got {digest}",
		pack.sha256
	);
	Ok(bytes.to_vec())
}

fn wanted(pack: &Pack, file: &str) -> Option<&'static str> {
	if file == LICENSE_FILE {
		return Some(LICENSE_OUT);
	}
	pack.libraries.iter().copied().find(|library| match pack.archive {
		Archive::Zip => file == *library,
		Archive::TarXz => file == *library || file.starts_with(&format!("{library}.")),
	})
}

fn extract(pack: &Pack, archive: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>> {
	let mut files: Vec<(&'static str, Vec<u8>)> = Vec::new();
	let mut keep = |name: &str, read: &mut dyn Read| -> Result<()> {
		let Some(target) = wanted(pack, name) else {
			return Ok(());
		};
		let mut bytes = Vec::new();
		read.read_to_end(&mut bytes)?;
		if !bytes.is_empty() && !files.iter().any(|(existing, _)| *existing == target) {
			files.push((target, bytes));
		}
		Ok(())
	};
	match pack.archive {
		Archive::Zip => {
			let mut zip = zip::ZipArchive::new(Cursor::new(archive)).context("codec pack is not a zip")?;
			for index in 0..zip.len() {
				let mut entry = zip.by_index(index)?;
				if !entry.is_file() {
					continue;
				}
				let name = entry.name()?.rsplit('/').next().unwrap_or_default().to_owned();
				keep(&name, &mut entry)?;
			}
		}
		Archive::TarXz => {
			let mut tar = Vec::new();
			lzma_rs::xz_decompress(&mut Cursor::new(archive), &mut tar).context("codec pack is not xz")?;
			let mut entries = tar::Archive::new(Cursor::new(tar));
			for entry in entries.entries()? {
				let mut entry = entry?;
				if !entry.header().entry_type().is_file() {
					continue;
				}
				let name = entry
					.path()?
					.file_name()
					.map(|name| name.to_string_lossy().into_owned())
					.unwrap_or_default();
				keep(&name, &mut entry)?;
			}
		}
	}
	for library in pack.libraries {
		ensure!(
			files.iter().any(|(name, _)| *name == library),
			"codec pack is missing {library}"
		);
	}
	Ok(files)
}
