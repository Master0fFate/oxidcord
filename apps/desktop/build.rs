use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
	if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
		return;
	}
	let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
	let root = manifest_dir.ancestors().nth(2).unwrap();
	let template_path = root.join("packaging").join("windows").join("oxidcord.rc");
	let icon_path = root.join("assets").join("icon.ico");
	println!("cargo:rerun-if-changed={}", template_path.display());
	println!("cargo:rerun-if-changed={}", icon_path.display());
	let version = |name: &str| env::var(name).unwrap();
	let script = fs::read_to_string(&template_path)
		.unwrap()
		.replace("@ICON@", &icon_path.to_string_lossy().replace('\\', "/"))
		.replace(
			"@VERSION_COMMA@",
			&format!(
				"{},{},{},0",
				version("CARGO_PKG_VERSION_MAJOR"),
				version("CARGO_PKG_VERSION_MINOR"),
				version("CARGO_PKG_VERSION_PATCH")
			),
		)
		.replace("@VERSION@", &version("CARGO_PKG_VERSION"));
	let output = PathBuf::from(env::var("OUT_DIR").unwrap()).join("oxidcord.rc");
	fs::write(&output, script).unwrap();
	embed_resource::compile(&output, embed_resource::NONE)
		.manifest_required()
		.unwrap();
}
