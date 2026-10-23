use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use plugin_host::Manifest;

const CARGO_TOML: &str = include_str!("../../templates/plugin/Cargo.toml.tmpl");
const PLUGIN_TOML: &str = include_str!("../../templates/plugin/plugin.toml.tmpl");
const LIB_RS: &str = include_str!("../../templates/plugin/lib.rs.tmpl");
const README: &str = include_str!("../../templates/plugin/README.md.tmpl");

#[derive(Args)]
pub struct NewArgs {
	#[arg(help = "Reverse dns id such as dev.example.my-plugin")]
	id: String,
	#[arg(long, default_value = "plugins")]
	dir: PathBuf,
}

struct Names {
	id: String,
	crate_name: String,
	wasm: String,
	title: String,
	type_name: String,
}

impl Names {
	fn from_id(id: &str) -> Result<Self> {
		let crate_name = id.rsplit('.').next().unwrap_or_default().to_owned();
		if !crate_name.starts_with(|first: char| first.is_ascii_lowercase()) {
			bail!("the last part of the id must start with a letter");
		}
		let words: Vec<String> = crate_name
			.split('-')
			.filter(|word| !word.is_empty())
			.map(capitalize)
			.collect();
		Ok(Self {
			id: id.to_owned(),
			wasm: format!("{}.wasm", crate_name.replace('-', "_")),
			title: words.join(" "),
			type_name: words.concat(),
			crate_name,
		})
	}

	fn render(&self, template: &str) -> String {
		template
			.replace("@id@", &self.id)
			.replace("@crate@", &self.crate_name)
			.replace("@wasm@", &self.wasm)
			.replace("@title@", &self.title)
			.replace("@struct@", &self.type_name)
	}
}

fn capitalize(word: &str) -> String {
	let mut chars = word.chars();
	chars
		.next()
		.map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
		.unwrap_or_default()
}

pub fn run(args: NewArgs) -> Result<()> {
	let names = Names::from_id(&args.id)?;
	let manifest = names.render(PLUGIN_TOML);
	Manifest::parse(&manifest).context("invalid plugin id")?;
	let root = args.dir.join(&names.crate_name);
	if root.exists() {
		bail!("{} already exists", root.display());
	}
	let source_dir = root.join("src");
	fs::create_dir_all(&source_dir).with_context(|| format!("could not create {}", source_dir.display()))?;
	let files = [
		(root.join("Cargo.toml"), names.render(CARGO_TOML)),
		(root.join("plugin.toml"), manifest),
		(source_dir.join("lib.rs"), names.render(LIB_RS)),
		(root.join("README.md"), names.render(README)),
	];
	for (path, contents) in files {
		fs::write(&path, contents).with_context(|| format!("could not write {}", path.display()))?;
	}
	println!("created {}", root.display());
	Ok(())
}
