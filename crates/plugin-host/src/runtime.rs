use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use store::Store;
use tokio::runtime::Handle;
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, OptLevel};

use crate::bindings::{Plugin, PluginPre};
use crate::broker::HostBroker;
use crate::config::RuntimeConfig;
use crate::error::{Error, Result};
use crate::fetch::Fetcher;
use crate::grants::Grants;
use crate::host_state::{HostState, Services};
use crate::slots::{Commands, Slots};

pub struct RuntimeShared {
	pub engine: Engine,
	pub linker: Linker<HostState>,
	pub services: Arc<Services>,
	pub config: RuntimeConfig,
	stop: Arc<AtomicBool>,
}

impl RuntimeShared {
	pub fn instance_pre(&self, component: &Component) -> Result<PluginPre<HostState>> {
		let pre = self.linker.instantiate_pre(component).map_err(Error::wasm)?;
		PluginPre::new(pre).map_err(Error::wasm)
	}
}

impl Drop for RuntimeShared {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Release);
	}
}

#[derive(Clone)]
pub struct Runtime {
	shared: Arc<RuntimeShared>,
}

impl Runtime {
	pub fn new(
		config: RuntimeConfig,
		handle: Handle,
		store: Arc<Store>,
		grants: Arc<Grants>,
		broker: Arc<dyn HostBroker>,
	) -> Result<Self> {
		let mut engine_config = Config::new();
		engine_config
			.epoch_interruption(true)
			.consume_fuel(false)
			.max_wasm_stack(config.max_wasm_stack)
			.cranelift_opt_level(OptLevel::Speed)
			.wasm_component_model(true);
		let engine = Engine::new(&engine_config).map_err(Error::wasm)?;

		let mut linker = Linker::new(&engine);
		wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(Error::wasm)?;
		Plugin::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)
			.map_err(Error::wasm)?;

		let services = Arc::new(Services {
			store,
			grants,
			broker,
			slots: Arc::new(Slots::default()),
			commands: Arc::new(Commands::default()),
			fetcher: Fetcher::new(handle),
		});
		let stop = Arc::new(AtomicBool::new(false));
		spawn_epoch_ticker(engine.clone(), Arc::clone(&stop), config.epoch_tick)?;
		Ok(Self {
			shared: Arc::new(RuntimeShared {
				engine,
				linker,
				services,
				config,
				stop,
			}),
		})
	}

	pub(crate) fn shared(&self) -> &Arc<RuntimeShared> {
		&self.shared
	}

	pub fn config(&self) -> &RuntimeConfig {
		&self.shared.config
	}

	pub fn grants(&self) -> &Arc<Grants> {
		&self.shared.services.grants
	}

	pub fn slots(&self) -> &Arc<Slots> {
		&self.shared.services.slots
	}

	pub fn commands(&self) -> &Arc<Commands> {
		&self.shared.services.commands
	}

	pub fn compile(&self, wasm: &[u8]) -> Result<Component> {
		Component::new(&self.shared.engine, wasm).map_err(Error::wasm)
	}
}

fn spawn_epoch_ticker(engine: Engine, stop: Arc<AtomicBool>, tick: std::time::Duration) -> Result<()> {
	thread::Builder::new()
		.name("plugin-epoch".to_owned())
		.spawn(move || {
			let mut next = Instant::now() + tick;
			while !stop.load(Ordering::Acquire) {
				let now = Instant::now();
				if now < next {
					thread::sleep(next - now);
				}
				let now = Instant::now();
				while next <= now {
					engine.increment_epoch();
					next += tick;
				}
			}
		})
		.map(|_| ())
		.map_err(|error| Error::io("plugin-epoch", error))
}
