use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use protocol::{Connection, User};
use rest::RestClient;
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};
use url::Url;

use crate::apps::Apps;
use crate::games::{self, GamesConfig};
use crate::rpc::{self, RpcShared};
use crate::spotify::{self, SpotifyAccount};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityRules {
	pub show_current_game: bool,
	pub rpc_server: bool,
	pub detect_games: bool,
	pub spotify: bool,
}

impl Default for ActivityRules {
	fn default() -> Self {
		Self {
			show_current_game: true,
			rpc_server: true,
			detect_games: true,
			spotify: true,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RpcEndpoints {
	pub ipc: Option<u8>,
	pub port: Option<u16>,
}

pub struct EngineConfig {
	pub rest: RestClient,
	pub rest_base: Url,
	pub user_agent: String,
	pub cache_dir: PathBuf,
	pub runtime: Handle,
	pub output: mpsc::UnboundedSender<Vec<Value>>,
}

pub(crate) enum Input {
	Rules(ActivityRules),
	User(Option<Box<User>>),
	RefreshConnections,
	Connections(Vec<Connection>),
	Rpc { link: u64, activity: Option<Value> },
	Game(Option<Value>),
	Spotify(Option<Value>),
}

#[derive(Clone)]
pub struct ActivityHandle {
	inputs: mpsc::UnboundedSender<Input>,
	endpoints: watch::Receiver<RpcEndpoints>,
}

impl ActivityHandle {
	pub fn set_rules(&self, rules: ActivityRules) {
		let _ = self.inputs.send(Input::Rules(rules));
	}

	pub fn set_user(&self, user: Option<User>) {
		let _ = self.inputs.send(Input::User(user.map(Box::new)));
	}

	pub fn refresh_connections(&self) {
		let _ = self.inputs.send(Input::RefreshConnections);
	}

	pub fn endpoints(&self) -> RpcEndpoints {
		*self.endpoints.borrow()
	}
}

pub struct Engine;

impl Engine {
	pub fn spawn(config: EngineConfig) -> ActivityHandle {
		let (inputs, input_receiver) = mpsc::unbounded_channel();
		let (endpoints_sender, endpoints) = watch::channel(RpcEndpoints::default());
		let (rpc_enabled, rpc_enabled_receiver) = watch::channel(false);
		let (games_enabled, games_enabled_receiver) = watch::channel(false);
		let (spotify_account, spotify_account_receiver) = watch::channel(None);
		let http = reqwest::Client::builder()
			.user_agent(config.user_agent.as_str())
			.build()
			.ok();
		let shared = Arc::new(RpcShared::new(Apps::new(config.rest.clone()), inputs.clone()));
		config
			.runtime
			.spawn(rpc::serve(shared.clone(), rpc_enabled_receiver, endpoints_sender));
		config.runtime.spawn(games::run(
			GamesConfig {
				rest_base: config.rest_base,
				http: http.clone(),
				cache_dir: config.cache_dir,
			},
			games_enabled_receiver,
			inputs.clone(),
		));
		if let Some(http) = http {
			config.runtime.spawn(spotify::run(
				config.rest.clone(),
				http,
				spotify_account_receiver,
				inputs.clone(),
			));
		}
		let state = EngineState {
			rest: config.rest,
			runtime: config.runtime.clone(),
			inputs: inputs.clone(),
			output: config.output,
			shared,
			rpc_enabled,
			games_enabled,
			spotify_account,
			rules: ActivityRules::default(),
			user: None,
			connections: Vec::new(),
			rpc: BTreeMap::new(),
			game: None,
			spotify: None,
			published: Vec::new(),
		};
		config.runtime.spawn(state.run(input_receiver));
		ActivityHandle { inputs, endpoints }
	}
}

struct EngineState {
	rest: RestClient,
	runtime: Handle,
	inputs: mpsc::UnboundedSender<Input>,
	output: mpsc::UnboundedSender<Vec<Value>>,
	shared: Arc<RpcShared>,
	rpc_enabled: watch::Sender<bool>,
	games_enabled: watch::Sender<bool>,
	spotify_account: watch::Sender<Option<SpotifyAccount>>,
	rules: ActivityRules,
	user: Option<User>,
	connections: Vec<Connection>,
	rpc: BTreeMap<u64, Value>,
	game: Option<Value>,
	spotify: Option<Value>,
	published: Vec<Value>,
}

impl EngineState {
	async fn run(mut self, mut inputs: mpsc::UnboundedReceiver<Input>) {
		self.apply_rules();
		while let Some(input) = inputs.recv().await {
			match input {
				Input::Rules(rules) => {
					if self.rules != rules {
						self.rules = rules;
						self.apply_rules();
					}
				}
				Input::User(user) => {
					self.user = user.map(|user| *user);
					self.shared.set_user(self.user.clone());
					self.apply_rules();
				}
				Input::RefreshConnections => self.fetch_connections(),
				Input::Connections(connections) => {
					self.connections = connections;
					self.apply_rules();
				}
				Input::Rpc { link, activity } => match activity {
					Some(activity) => {
						self.rpc.insert(link, activity);
					}
					None => {
						self.rpc.remove(&link);
					}
				},
				Input::Game(game) => self.game = game,
				Input::Spotify(activity) => self.spotify = activity,
			}
			self.publish();
		}
	}

	fn apply_rules(&mut self) {
		self.rpc_enabled.send_replace(self.rules.rpc_server);
		self.games_enabled
			.send_replace(self.rules.show_current_game && self.rules.detect_games);
		self.spotify_account.send_replace(self.spotify_account());
	}

	fn spotify_account(&self) -> Option<SpotifyAccount> {
		if !self.rules.spotify {
			return None;
		}
		let user_id = self.user.as_ref()?.id;
		self.connections
			.iter()
			.find(|connection| connection.is_spotify() && connection.show_activity)
			.and_then(|connection| {
				Some(SpotifyAccount {
					connection_id: connection.id.clone(),
					token: connection.access_token.as_ref()?.as_str().to_owned(),
					user_id,
				})
			})
	}

	fn fetch_connections(&self) {
		let rest = self.rest.clone();
		let inputs = self.inputs.clone();
		self.runtime.spawn(async move {
			match rest.connections().await {
				Ok(connections) => {
					let _ = inputs.send(Input::Connections(connections.into_vec()));
				}
				Err(error) => tracing::debug!(%error, "connections were not loaded"),
			}
		});
	}

	fn compose(&self) -> Vec<Value> {
		let mut activities = Vec::new();
		if let Some(spotify) = &self.spotify {
			activities.push(spotify.clone());
		}
		if !self.rules.show_current_game {
			return activities;
		}
		if self.rules.rpc_server {
			activities.extend(self.rpc.values().cloned());
		}
		if self.rules.detect_games
			&& let Some(game) = &self.game
		{
			let application = game.get("application_id");
			let covered = self
				.rpc
				.values()
				.any(|activity| activity.get("application_id") == application);
			if !covered {
				activities.push(game.clone());
			}
		}
		activities
	}

	fn publish(&mut self) {
		let activities = self.compose();
		if activities == self.published {
			return;
		}
		self.published = activities.clone();
		let _ = self.output.send(activities);
	}
}
