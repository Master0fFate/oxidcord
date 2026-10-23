use plugin_sdk::{Event, HostError, OutgoingMessage, Plugin, export_plugin, host};

const SPIN_KEY: &str = "oxidcord.conformance.spin";
const SEND_COMMAND: &str = "oxidcord.conformance.send";
const CONTEXT_COMMAND: &str = "oxidcord.conformance.context";
const ALLOC_KEY: &str = "oxidcord.conformance.alloc";
const ALLOC_BYTES: usize = 256 * 1024 * 1024;

#[derive(Default)]
struct ConformanceProbe;

impl Plugin for ConformanceProbe {
	fn on_event(&mut self, event: Event) {
		let Event::SettingChanged(change) = event else {
			return;
		};
		match change.key.as_str() {
			SPIN_KEY => loop {
				std::hint::black_box(());
			},
			ALLOC_KEY => {
				let mut hog = Vec::<u8>::with_capacity(ALLOC_BYTES);
				hog.resize(ALLOC_BYTES, 1);
				std::hint::black_box(hog);
			}
			_ => {}
		}
	}

	fn on_command(&mut self, name: &str, _args: &str, channel_id: u64) -> Result<(), String> {
		match name {
			SEND_COMMAND => send(channel_id),
			CONTEXT_COMMAND => Err(probe_context()),
			_ => Ok(()),
		}
	}
}

fn outcome<T>(name: &str, result: Result<T, HostError>) -> String {
	match result {
		Ok(_) => format!("{name}:Ok"),
		Err(error) => format!("{name}:{error:?}"),
	}
}

fn probe_context() -> String {
	[
		outcome("list-guilds", host::list_guilds()),
		outcome("list-channels", host::list_channels(Some(1))),
		outcome("list-members", host::list_members(1, 1)),
		outcome("voice-state", host::voice_state()),
	]
	.join(" ")
}

fn send(channel_id: u64) -> Result<(), String> {
	let message = OutgoingMessage {
		channel_id,
		content: "conformance probe".to_owned(),
		reply_to: None,
	};
	host::send_message(&message)
		.map(drop)
		.map_err(|error| format!("{error:?}"))
}

export_plugin!(ConformanceProbe);
