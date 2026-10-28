use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use store::Store;
use tokio::runtime::Handle;
use wasmtime::component::{Component, HasSelf, InstancePre, Linker};
use wasmtime::{Config, Engine, OptLevel};

use crate::bindings::PluginIndices;
use crate::bindings::ext::PluginExt;
use crate::bindings::ext::exports::oxidcord::plugin::hooks::GuestIndices as HookIndices;
use crate::bindings::ext::exports::oxidcord::plugin::hooks2::GuestIndices as Hooks2Indices;
use crate::broker::HostBroker;
use crate::config::RuntimeConfig;
use crate::error::{Error, Result};
use crate::fetch::Fetcher;
use crate::grants::Grants;
use crate::host_state::{HostState, Services};
use crate::latency::LatencyStats;
use crate::mounts::Mounts;
use crate::patches::Patches;
use crate::slots::{Commands, Slots};
use crate::timers::Timers;

const PACKAGE: &str = "oxidcord:plugin";

#[derive(Clone)]
pub(crate) struct Prepared {
	pub(crate) instance: InstancePre<HostState>,
	pub(crate) guest: PluginIndices,
	pub(crate) hooks: Option<HookIndices>,
	pub(crate) hooks2: Option<Hooks2Indices>,
}

#[derive(Default)]
pub(crate) struct Activity {
	running: AtomicUsize,
	ticker: OnceLock<Thread>,
}

pub(crate) struct Busy<'a>(&'a Activity);

impl Activity {
	pub(crate) fn enter(&self) -> Busy<'_> {
		if self.running.fetch_add(1, Ordering::AcqRel) == 0 {
			self.wake();
		}
		Busy(self)
	}

	fn busy(&self) -> bool {
		self.running.load(Ordering::Acquire) > 0
	}

	fn wake(&self) {
		if let Some(ticker) = self.ticker.get() {
			ticker.unpark();
		}
	}
}

impl Drop for Busy<'_> {
	fn drop(&mut self) {
		self.0.running.fetch_sub(1, Ordering::AcqRel);
	}
}

pub struct RuntimeShared {
	pub engine: Engine,
	pub linker: Linker<HostState>,
	pub services: Arc<Services>,
	pub config: RuntimeConfig,
	pub(crate) activity: Arc<Activity>,
	stop: Arc<AtomicBool>,
}

impl RuntimeShared {
	pub(crate) fn prepare(&self, component: &Component) -> Result<Prepared> {
		let instance = self.linker.instantiate_pre(component).map_err(Error::wasm)?;
		let guest = PluginIndices::new(&instance).map_err(Error::wasm)?;
		let hooks = self.optional(component, "hooks", HookIndices::new(&instance))?;
		let hooks2 = self.optional(component, "hooks2", Hooks2Indices::new(&instance))?;
		Ok(Prepared {
			instance,
			guest,
			hooks,
			hooks2,
		})
	}

	fn optional<T>(
		&self,
		component: &Component,
		interface: &str,
		indices: wasmtime::Result<T>,
	) -> Result<Option<T>> {
		match indices {
			Ok(indices) => Ok(Some(indices)),
			Err(error) if self.exports_interface(component, interface) => Err(Error::Incompatible(format!(
				"exported interface {interface} does not match api {}: {error:#}",
				crate::manifest::API_VERSION
			))),
			Err(_) => Ok(None),
		}
	}

	fn exports_interface(&self, component: &Component, interface: &str) -> bool {
		let prefix = format!("{PACKAGE}/{interface}@");
		component
			.component_type()
			.exports(&self.engine)
			.any(|(name, _)| name.starts_with(&prefix))
	}
}

impl Drop for RuntimeShared {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Release);
		self.activity.wake();
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
		PluginExt::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state)
			.map_err(Error::wasm)?;

		let services = Arc::new(Services {
			store,
			grants,
			broker,
			slots: Arc::new(Slots::default()),
			commands: Arc::new(Commands::default()),
			mounts: Arc::new(Mounts::default()),
			patches: Arc::new(Patches::default()),
			timers: Timers::spawn().map_err(|error| Error::io("plugin-timers", error))?,
			fetcher: Fetcher::new(handle),
			latency: LatencyStats::default(),
		});
		let stop = Arc::new(AtomicBool::new(false));
		let activity = Arc::new(Activity::default());
		spawn_epoch_ticker(
			engine.clone(),
			Arc::clone(&stop),
			Arc::clone(&activity),
			config.epoch_tick,
		)?;
		Ok(Self {
			shared: Arc::new(RuntimeShared {
				engine,
				linker,
				services,
				config,
				activity,
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

	pub fn mounts(&self) -> &Arc<Mounts> {
		&self.shared.services.mounts
	}

	pub fn patches(&self) -> &Arc<Patches> {
		&self.shared.services.patches
	}

	pub(crate) fn latency(&self) -> &LatencyStats {
		&self.shared.services.latency
	}

	pub(crate) fn timers(&self) -> &Timers {
		&self.shared.services.timers
	}

	pub fn compile(&self, wasm: &[u8]) -> Result<Component> {
		Component::new(&self.shared.engine, wasm).map_err(Error::wasm)
	}
}

fn spawn_epoch_ticker(
	engine: Engine,
	stop: Arc<AtomicBool>,
	activity: Arc<Activity>,
	tick: Duration,
) -> Result<()> {
	let worker = Arc::clone(&activity);
	let handle = thread::Builder::new()
		.name("plugin-epoch".to_owned())
		.spawn(move || {
			let mut next = Instant::now() + tick;
			while !stop.load(Ordering::Acquire) {
				if !worker.busy() {
					thread::park();
					next = Instant::now() + tick;
					continue;
				}
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
		.map_err(|error| Error::io("plugin-epoch", error))?;
	let _ = activity.ticker.set(handle.thread().clone());
	Ok(())
}
