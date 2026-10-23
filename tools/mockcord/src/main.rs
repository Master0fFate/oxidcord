use std::process::ExitCode;
use std::str::FromStr;

use mockcord::{MockConfig, Server};
use tracing_subscriber::EnvFilter;

const DEFAULT_PORT: u16 = 8485;

fn next_value<T: FromStr>(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<T, String> {
	args.next()
		.and_then(|value| value.parse().ok())
		.ok_or_else(|| format!("{flag} needs a numeric value"))
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<MockConfig, String> {
	let mut config = MockConfig {
		port: Some(DEFAULT_PORT),
		..MockConfig::default()
	};
	while let Some(flag) = args.next() {
		match flag.as_str() {
			"--guilds" => config.guilds = next_value(&mut args, &flag)?,
			"--channels-per-guild" => config.channels_per_guild = next_value(&mut args, &flag)?,
			"--messages-per-channel" => config.messages_per_channel = next_value(&mut args, &flag)?,
			"--members-per-guild" => config.members_per_guild = next_value(&mut args, &flag)?,
			"--seed" => config.seed = next_value(&mut args, &flag)?,
			"--port" => config.port = Some(next_value(&mut args, &flag)?),
			"--chaos" => config.chaos = true,
			"--live-traffic" => config.live_traffic = true,
			other => return Err(format!("unknown flag {other}")),
		}
	}
	Ok(config)
}

#[tokio::main]
async fn main() -> ExitCode {
	tracing_subscriber::fmt()
		.with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
		.with_writer(std::io::stdout)
		.init();
	let config = match parse_args(std::env::args().skip(1)) {
		Ok(config) => config,
		Err(message) => {
			eprintln!("mockcord: {message}");
			return ExitCode::from(2);
		}
	};
	let server = match Server::start(config).await {
		Ok(server) => server,
		Err(error) => {
			eprintln!("mockcord: cannot start: {error}");
			return ExitCode::FAILURE;
		}
	};
	let summary = serde_json::to_string(&server.fixtures().summary()).unwrap_or_default();
	tracing::info!(
		rest = server.rest_base_url(),
		gateway = server.gateway_url(),
		"mockcord listening"
	);
	tracing::info!(fixtures = %summary, "fixtures ready");
	let _ = tokio::signal::ctrl_c().await;
	server.shutdown().await;
	ExitCode::SUCCESS
}
