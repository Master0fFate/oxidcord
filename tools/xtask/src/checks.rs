use std::env;
use std::process::Command;

use anyhow::{Context, Result, bail};

fn cargo(args: &[&str]) -> Result<()> {
	let program = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
	let status = Command::new(&program)
		.args(args)
		.status()
		.with_context(|| format!("could not start {program}"))?;
	if !status.success() {
		bail!("cargo {} failed", args.join(" "));
	}
	Ok(())
}

pub fn fmt_check() -> Result<()> {
	cargo(&["fmt", "--all", "--", "--check"])
}

pub fn lint() -> Result<()> {
	cargo(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])
}

pub fn check_all() -> Result<()> {
	fmt_check()?;
	lint()?;
	cargo(&["build"])
}
