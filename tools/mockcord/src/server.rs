use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::MockConfig;
use crate::fixtures::Fixtures;
use crate::rest;
use crate::state::{Shared, Urls};
use crate::traffic;
use crate::voice;

const ANCHOR_GRANULARITY_MS: i64 = 3_600_000;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

pub struct Server {
	shared: Arc<Shared>,
	address: SocketAddr,
	stop: watch::Sender<bool>,
	tasks: Vec<JoinHandle<()>>,
}

fn fixture_anchor() -> DateTime<Utc> {
	let now_ms = Utc::now().timestamp_millis();
	DateTime::from_timestamp_millis(now_ms - now_ms % ANCHOR_GRANULARITY_MS).unwrap_or_default()
}

impl Server {
	pub async fn start(config: MockConfig) -> io::Result<Server> {
		let listener = TcpListener::bind(("127.0.0.1", config.port.unwrap_or(0))).await?;
		let address = listener.local_addr()?;
		let fixtures = Fixtures::generate(&config, fixture_anchor());
		let (stop, stopped) = watch::channel(false);
		let voice = voice::spawn(stopped.clone()).await?;
		let urls = Urls {
			rest: format!("http://{address}/api/v10"),
			gateway: format!("ws://{address}/gateway"),
			voice: voice.url,
		};
		let shared = Arc::new(Shared::new(config, fixtures, urls, stopped.clone()));
		let mut graceful = stopped;
		let serve = axum::serve(listener, rest::router(shared.clone())).with_graceful_shutdown(async move {
			let _ = graceful.wait_for(|stopped| *stopped).await;
		});
		let mut tasks = vec![tokio::spawn(async move {
			if let Err(error) = serve.await {
				tracing::error!(%error, "mock server stopped");
			}
		})];
		tasks.extend(voice.tasks);
		tasks.extend(traffic::spawn(&shared));
		Ok(Server {
			shared,
			address,
			stop,
			tasks,
		})
	}

	pub fn rest_base_url(&self) -> &str {
		&self.shared.urls.rest
	}

	pub fn gateway_url(&self) -> &str {
		&self.shared.urls.gateway
	}

	pub fn address(&self) -> SocketAddr {
		self.address
	}

	pub fn token(&self) -> &str {
		&self.shared.config.token
	}

	pub fn fixtures(&self) -> &Fixtures {
		&self.shared.fixtures
	}

	pub async fn shutdown(mut self) {
		let _ = self.stop.send(true);
		for mut task in self.tasks.drain(..) {
			if tokio::time::timeout(SHUTDOWN_GRACE, &mut task).await.is_err() {
				task.abort();
			}
		}
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		let _ = self.stop.send(true);
		for task in &self.tasks {
			task.abort();
		}
	}
}
