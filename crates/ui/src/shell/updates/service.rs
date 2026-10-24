use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AppContext as _, Context, Entity, Global};
use store::Store;
use tokio::runtime::Handle;
use tokio::sync::mpsc::unbounded_channel;
use updater::{AvailableUpdate, Channel, InstallKind, Staged, Updater, UpdaterConfig, UpdaterError};

const REPOSITORY_HOST: &str = "https://github.com/";
const RECHECK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

pub const STABLE_CHANNEL: &str = "stable";
pub const BETA_CHANNEL: &str = "beta";

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
	Idle,
	Checking,
	UpToDate,
	CouldNotCheck,
	Available {
		version: String,
		notes_url: String,
		installable: bool,
	},
	Downloading {
		version: String,
		done: u64,
		total: u64,
	},
	Ready {
		version: String,
	},
	Installing,
	Failed(String),
}

enum Download {
	Progress(u64, u64),
	Finished(Result<Staged, UpdaterError>),
}

pub struct UpdateService {
	store: Arc<Store>,
	runtime: Handle,
	status: UpdateStatus,
	updater: Option<Arc<Updater>>,
	update: Option<AvailableUpdate>,
	staged: Option<Staged>,
}

pub struct UpdateHandle(pub Entity<UpdateService>);

impl Global for UpdateHandle {}

impl UpdateService {
	pub fn launch(store: Arc<Store>, check_now: bool, cx: &mut App) {
		let service = cx.new(|_| Self {
			store,
			runtime: Handle::current(),
			status: UpdateStatus::Idle,
			updater: None,
			update: None,
			staged: None,
		});
		cx.set_global(UpdateHandle(service.clone()));
		if check_now {
			service.update(cx, |service, cx| service.check(cx));
			let weak = service.downgrade();
			cx.spawn(async move |cx| {
				loop {
					cx.background_executor().timer(RECHECK_INTERVAL).await;
					if weak.update(cx, |service, cx| service.recheck(cx)).is_err() {
						break;
					}
				}
			})
			.detach();
		}
	}

	pub fn status(&self) -> &UpdateStatus {
		&self.status
	}

	pub fn is_busy(&self) -> bool {
		matches!(
			self.status,
			UpdateStatus::Checking
				| UpdateStatus::Downloading { .. }
				| UpdateStatus::Ready { .. }
				| UpdateStatus::Installing
		)
	}

	pub fn is_ready(&self) -> bool {
		matches!(self.status, UpdateStatus::Ready { .. })
	}

	fn set_status(&mut self, status: UpdateStatus, cx: &mut Context<Self>) {
		self.status = status;
		cx.notify();
	}

	fn build_updater(&self) -> Option<Arc<Updater>> {
		let repository = env!("CARGO_PKG_REPOSITORY").trim_start_matches(REPOSITORY_HOST);
		let data_dir = self.store.paths.data_dir().to_path_buf();
		let built = UpdaterConfig::new(repository, data_dir).and_then(|mut config| {
			if self.store.settings.read().updates.channel == BETA_CHANNEL {
				config.channel = Channel::Beta;
			}
			Updater::new(config)
		});
		match built {
			Ok(updater) => Some(Arc::new(updater)),
			Err(error) => {
				tracing::warn!(%error, "updater is unavailable");
				None
			}
		}
	}

	fn recheck(&mut self, cx: &mut Context<Self>) {
		if !matches!(self.status, UpdateStatus::Available { .. }) {
			self.check(cx);
		}
	}

	pub fn act(&mut self, cx: &mut Context<Self>) {
		match &self.status {
			UpdateStatus::Available {
				installable: true, ..
			} => self.download(cx),
			UpdateStatus::Available { notes_url, .. } => {
				if let Err(error) = platform::open::open_url(notes_url) {
					tracing::warn!(%error, "could not open the release notes");
				}
			}
			UpdateStatus::Ready { .. } => self.restart(cx),
			_ => {}
		}
	}

	pub fn check(&mut self, cx: &mut Context<Self>) {
		if self.is_busy() {
			return;
		}
		let Some(updater) = self.build_updater() else {
			self.set_status(UpdateStatus::CouldNotCheck, cx);
			return;
		};
		self.set_status(UpdateStatus::Checking, cx);
		let job = {
			let updater = updater.clone();
			self.runtime.spawn(async move { updater.check().await })
		};
		cx.spawn(async move |this, cx| {
			let outcome = job.await;
			let _ = this.update(cx, |this, cx| match outcome {
				Ok(Ok(Some(update))) => this.found(updater, update, cx),
				Ok(Ok(None)) => this.set_status(UpdateStatus::UpToDate, cx),
				Ok(Err(error)) => {
					tracing::info!(%error, "update check failed");
					this.set_status(UpdateStatus::CouldNotCheck, cx);
				}
				Err(error) => {
					tracing::warn!(%error, "update check was interrupted");
					this.set_status(UpdateStatus::CouldNotCheck, cx);
				}
			});
		})
		.detach();
	}

	fn found(&mut self, updater: Arc<Updater>, update: AvailableUpdate, cx: &mut Context<Self>) {
		let status = UpdateStatus::Available {
			version: update.version.to_string(),
			notes_url: update.notes_url.clone(),
			installable: update.artifact.is_some(),
		};
		self.updater = Some(updater);
		self.update = Some(update);
		self.set_status(status, cx);
	}

	pub fn download(&mut self, cx: &mut Context<Self>) {
		let (UpdateStatus::Available { .. }, Some(updater), Some(update)) =
			(&self.status, self.updater.clone(), self.update.clone())
		else {
			return;
		};
		let Some(total) = update.artifact.as_ref().map(|artifact| artifact.length) else {
			return;
		};
		self.set_status(
			UpdateStatus::Downloading {
				version: update.version.to_string(),
				done: 0,
				total,
			},
			cx,
		);
		let (sender, mut receiver) = unbounded_channel();
		let progress = sender.clone();
		self.runtime.spawn(async move {
			let result = updater
				.download(&update, move |done, total| {
					let _ = progress.send(Download::Progress(done, total));
				})
				.await;
			let _ = sender.send(Download::Finished(result));
		});
		cx.spawn(async move |this, cx| {
			while let Some(mut message) = receiver.recv().await {
				while let Ok(next) = receiver.try_recv() {
					message = next;
				}
				let finished = matches!(message, Download::Finished(_));
				let delivered = this.update(cx, |this, cx| this.downloaded(message, cx));
				if finished || delivered.is_err() {
					break;
				}
			}
		})
		.detach();
	}

	fn downloaded(&mut self, message: Download, cx: &mut Context<Self>) {
		match message {
			Download::Progress(done, total) => {
				if let UpdateStatus::Downloading {
					done: current_done,
					total: current_total,
					..
				} = &mut self.status
				{
					*current_done = done;
					*current_total = total;
					cx.notify();
				}
			}
			Download::Finished(Ok(staged)) => {
				let status = UpdateStatus::Ready {
					version: staged.version().to_string(),
				};
				self.staged = Some(staged);
				self.set_status(status, cx);
			}
			Download::Finished(Err(error)) => {
				tracing::warn!(%error, "update download failed");
				self.set_status(UpdateStatus::Failed(error.to_string()), cx);
			}
		}
	}

	pub fn restart(&mut self, cx: &mut Context<Self>) {
		if !self.is_ready() {
			return;
		}
		let (Some(updater), Some(staged)) = (self.updater.clone(), self.staged.take()) else {
			return;
		};
		let relaunch = updater.install_env().kind != Some(InstallKind::Installer);
		self.set_status(UpdateStatus::Installing, cx);
		let job = self.runtime.spawn_blocking(move || updater.install(&staged));
		cx.spawn(async move |this, cx| {
			let failure = match job.await {
				Ok(Ok(_)) if relaunch => platform::relaunch::current_exe()
					.err()
					.map(|error| error.to_string()),
				Ok(Ok(_)) => None,
				Ok(Err(error)) => Some(error.to_string()),
				Err(error) => Some(error.to_string()),
			};
			match failure {
				None => {
					cx.update(|cx| cx.quit());
				}
				Some(error) => {
					tracing::warn!(%error, "update install failed");
					let _ = this.update(cx, |this, cx| this.set_status(UpdateStatus::Failed(error), cx));
				}
			}
		})
		.detach();
	}
}
