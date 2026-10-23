mod checks;
mod headless;
mod plugin;
mod release;
mod theme;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "Oxidcord development tasks")]
struct Cli {
	#[command(subcommand)]
	task: Task,
}

#[derive(Subcommand)]
enum Task {
	HeadlessRun(headless::Args),
	Plugin(plugin::PluginArgs),
	Release(release::ReleaseArgs),
	Theme(theme::ThemeArgs),
	FmtCheck,
	Lint,
	CheckAll,
}

fn main() -> ExitCode {
	let cli = Cli::parse();
	let outcome = match cli.task {
		Task::HeadlessRun(args) => headless::run(args),
		Task::Plugin(args) => plugin::run(args),
		Task::Release(args) => release::run(args),
		Task::Theme(args) => theme::run(args),
		Task::FmtCheck => checks::fmt_check(),
		Task::Lint => checks::lint(),
		Task::CheckAll => checks::check_all(),
	};
	match outcome {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("xtask: {error:#}");
			ExitCode::FAILURE
		}
	}
}
