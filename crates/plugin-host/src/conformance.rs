use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use store::{Paths, Store};

use crate::api::model::{
	ChannelClick, ChannelDetail, ClickKind, ClickTarget, ClickWatch, ClientOption, DmSection, EmojiInfo,
	EmojiRef, Keybind, Lifecycle, MessageClick, MessageFull, MessageMeta, Modifiers, NotificationInfo,
	NotificationKind, NotificationSettings, OutgoingFull, UiContext,
};
use crate::api::{
	Event, FetchRequest, HostError, MessageRef, MessageView, OutgoingMessage, SettingChange, UiTree, User,
};
use crate::bindings::ext::oxidcord::plugin::client::Host as ClientHost;
use crate::bindings::ext::oxidcord::plugin::ui::Host as UiHost;
use crate::bindings::oxidcord::plugin::host::Host;
use crate::broker::NoopBroker;
use crate::config::RuntimeConfig;
use crate::convert::{md_to_rich, md_to_tree};
use crate::grants::{Grants, PluginGrant};
use crate::hook::Hook;
use crate::host_state::HostState;
use crate::instance::{CallError, CallKind, Output, PluginInstance};
use crate::manager::PluginManager;
use crate::manifest::Manifest;
use crate::package::{Package, discover};
use crate::permission::Permission;
use crate::runtime::Runtime;

pub const PROBE_SPIN_KEY: &str = "oxidcord.conformance.spin";
pub const PROBE_ALLOC_KEY: &str = "oxidcord.conformance.alloc";
pub const PROBE_SEND_COMMAND: &str = "oxidcord.conformance.send";
pub const PROBE_CONTEXT_COMMAND: &str = "oxidcord.conformance.context";

const SCRATCH_REMOVE_ATTEMPTS: u32 = 20;
const SCRATCH_REMOVE_PAUSE: Duration = Duration::from_millis(100);
const GATE_PLUGIN: &str = "conformance.gate";
const RESTART_WAIT: Duration = Duration::from_secs(5);
const DEADLINE_SLACK: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
	Pass,
	Fail,
	Skipped,
}

#[derive(Debug, Clone)]
pub struct Check {
	pub name: String,
	pub outcome: Outcome,
	pub elapsed: Duration,
	pub detail: String,
}

#[derive(Debug, Clone)]
pub struct PluginReport {
	pub id: String,
	pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
	pub plugins: Vec<PluginReport>,
}

impl Report {
	pub fn passed(&self) -> bool {
		self.plugins
			.iter()
			.flat_map(|plugin| &plugin.checks)
			.all(|check| check.outcome != Outcome::Fail)
	}
}

impl fmt::Display for Report {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		for plugin in &self.plugins {
			writeln!(formatter, "{}", plugin.id)?;
			for check in &plugin.checks {
				let mark = match check.outcome {
					Outcome::Pass => "pass",
					Outcome::Fail => "FAIL",
					Outcome::Skipped => "skip",
				};
				writeln!(
					formatter,
					"  [{mark}] {} ({:.1} ms) {}",
					check.name,
					check.elapsed.as_secs_f64() * 1000.0,
					check.detail
				)?;
			}
		}
		writeln!(
			formatter,
			"{}",
			if self.passed() {
				"conformance passed"
			} else {
				"conformance FAILED"
			}
		)
	}
}

struct Recorder {
	checks: Vec<Check>,
}

impl Recorder {
	fn new() -> Self {
		Self { checks: Vec::new() }
	}

	fn push(&mut self, name: &str, outcome: Outcome, elapsed: Duration, detail: impl Into<String>) {
		self.checks.push(Check {
			name: name.to_owned(),
			outcome,
			elapsed,
			detail: detail.into(),
		});
	}

	fn pass(&mut self, name: &str, elapsed: Duration, detail: impl Into<String>) {
		self.push(name, Outcome::Pass, elapsed, detail);
	}

	fn fail(&mut self, name: &str, elapsed: Duration, detail: impl Into<String>) {
		self.push(name, Outcome::Fail, elapsed, detail);
	}

	fn skip(&mut self, name: &str, detail: impl Into<String>) {
		self.push(name, Outcome::Skipped, Duration::ZERO, detail);
	}
}

struct Sandbox {
	root: PathBuf,
	tokio: tokio::runtime::Runtime,
	runtime: Runtime,
}

impl Sandbox {
	fn close(self) {
		drop(self.runtime);
		drop(self.tokio);
		remove_scratch(&self.root);
	}
}

pub fn remove_scratch(root: &Path) {
	for _ in 0..SCRATCH_REMOVE_ATTEMPTS {
		match std::fs::remove_dir_all(root) {
			Ok(()) => return,
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
			Err(_) => std::thread::sleep(SCRATCH_REMOVE_PAUSE),
		}
	}
}

fn sandbox() -> Result<Sandbox, String> {
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_nanos());
	let root = std::env::temp_dir().join(format!("oxidcord-conformance-{}-{nanos}", std::process::id()));
	let paths = Paths::at(&root).map_err(|error| error.to_string())?;
	let store = Arc::new(Store::open(paths).map_err(|error| error.to_string())?);
	let tokio = tokio::runtime::Builder::new_multi_thread()
		.worker_threads(1)
		.enable_all()
		.build()
		.map_err(|error| error.to_string())?;
	let grants = Arc::new(Grants::new(Arc::clone(&store)));
	let runtime = Runtime::new(
		RuntimeConfig::default(),
		tokio.handle().clone(),
		store,
		grants,
		Arc::new(NoopBroker),
	)
	.map_err(|error| error.to_string())?;
	Ok(Sandbox { root, tokio, runtime })
}

pub fn run(plugins_dir: &Path) -> Report {
	let mut report = Report::default();
	let sandbox = match sandbox() {
		Ok(sandbox) => sandbox,
		Err(error) => {
			let mut recorder = Recorder::new();
			recorder.fail("sandbox setup", Duration::ZERO, error);
			report.plugins.push(PluginReport {
				id: "host".to_owned(),
				checks: recorder.checks,
			});
			return report;
		}
	};
	report.plugins.push(PluginReport {
		id: "host".to_owned(),
		checks: check_host_gating(&sandbox.runtime),
	});
	for dir in discover(plugins_dir) {
		report.plugins.push(check_plugin(&sandbox.runtime, &dir));
	}
	sandbox.close();
	report
}

fn denied_matches(result: Result<(), HostError>, expected: Permission) -> bool {
	matches!(result, Err(error) if error == expected.denied())
}

fn check_host_gating(runtime: &Runtime) -> Vec<Check> {
	let mut recorder = Recorder::new();
	let started = Instant::now();
	let mut state = HostState::new(
		Arc::from(GATE_PLUGIN),
		Arc::clone(&runtime.shared().services),
		runtime.config(),
	);
	let message_ref = MessageRef {
		id: 1,
		channel_id: 2,
		guild_id: None,
	};
	let keybind = Keybind {
		id: "probe".to_owned(),
		chord: "ctrl+k".to_owned(),
	};
	let outgoing = OutgoingMessage {
		channel_id: 2,
		content: "probe".to_owned(),
		reply_to: None,
	};
	let fetch = FetchRequest {
		method: "GET".to_owned(),
		url: "https://example.com/".to_owned(),
		headers: Vec::new(),
		body: None,
	};
	let empty_tree = UiTree {
		nodes: Vec::new(),
		roots: Vec::new(),
	};
	let emoji = EmojiRef {
		id: Some(5),
		name: "probe".to_owned(),
		animated: false,
	};
	let settings = NotificationSettings {
		muted: None,
		level: None,
		suppress_everyone: None,
		suppress_roles: None,
		mobile_push: None,
		hide_muted: None,
		mute_until_ms: None,
	};
	let watch = ClickWatch {
		kinds: Vec::new(),
		modifiers: Modifiers::empty(),
		targets: Vec::new(),
	};
	let results: Vec<(&str, Permission, Result<(), HostError>)> = vec![
		(
			"storage-get",
			Permission::Storage,
			state.storage_get("k".to_owned()).map(drop),
		),
		(
			"storage-set",
			Permission::Storage,
			state.storage_set("k".to_owned(), Vec::new()),
		),
		(
			"storage-delete",
			Permission::Storage,
			state.storage_delete("k".to_owned()),
		),
		(
			"storage-list",
			Permission::Storage,
			state.storage_list().map(drop),
		),
		("fetch", Permission::NetFetch, state.fetch(fetch).map(drop)),
		(
			"send-message",
			Permission::ActionsSend,
			state.send_message(outgoing.clone()).map(drop),
		),
		(
			"add-reaction",
			Permission::ActionsReact,
			state.add_reaction(message_ref, "x".to_owned()),
		),
		(
			"open-channel",
			Permission::ActionsNavigate,
			state.open_channel(None, 2),
		),
		(
			"clipboard-write",
			Permission::ActionsClipboard,
			state.clipboard_write("x".to_owned()),
		),
		(
			"notify",
			Permission::ActionsNotify,
			state.notify("t".to_owned(), "b".to_owned()),
		),
		(
			"open-url",
			Permission::ActionsOpenUrl,
			state.open_url("https://example.com/".to_owned()),
		),
		(
			"current-user",
			Permission::ContextBasic,
			state.current_user().map(drop),
		),
		(
			"current-channel",
			Permission::ContextBasic,
			state.current_channel().map(drop),
		),
		(
			"channel-messages",
			Permission::ContextTimeline,
			state.channel_messages(2, 10).map(drop),
		),
		(
			"display-name",
			Permission::ContextMembers,
			state.display_name(None, 4).map(drop),
		),
		(
			"list-guilds",
			Permission::ContextDirectory,
			state.list_guilds().map(drop),
		),
		(
			"list-channels",
			Permission::ContextDirectory,
			state.list_channels(Some(3)).map(drop),
		),
		(
			"list-members",
			Permission::ContextDirectory,
			state.list_members(3, 10).map(drop),
		),
		(
			"voice-state",
			Permission::ContextVoice,
			state.voice_state().map(drop),
		),
		(
			"register-slot",
			Permission::UiSlots,
			state.register_slot("probe".to_owned(), empty_tree),
		),
		(
			"register-command",
			Permission::UiSlots,
			state.register_command("probe".to_owned(), String::new()),
		),
		(
			"list-relationships",
			Permission::ContextRelationships,
			state.list_relationships().map(drop),
		),
		(
			"user-presence",
			Permission::ContextPresence,
			state.user_presence(4).map(drop),
		),
		(
			"idle-seconds",
			Permission::ContextPresence,
			state.idle_seconds().map(drop),
		),
		(
			"read-state",
			Permission::ContextReadState,
			state.read_state(2).map(drop),
		),
		(
			"unread-channels",
			Permission::ContextReadState,
			state.unread_channels(None).map(drop),
		),
		(
			"channel-muted",
			Permission::ContextReadState,
			state.channel_muted(2).map(drop),
		),
		(
			"list-roles",
			Permission::ContextRoles,
			state.list_roles(3).map(drop),
		),
		(
			"member-info",
			Permission::ContextRoles,
			state.member_info(3, 4).map(drop),
		),
		(
			"channel-permissions",
			Permission::ContextRoles,
			state.channel_permissions(2, None).map(drop),
		),
		(
			"guild-detail",
			Permission::ContextDirectory,
			state.guild_detail(3).map(drop),
		),
		(
			"channel-detail",
			Permission::ContextDirectory,
			state.channel_detail(2).map(drop),
		),
		(
			"list-folders",
			Permission::ContextDirectory,
			state.list_folders().map(drop),
		),
		(
			"user-profile",
			Permission::ContextProfiles,
			state.user_profile(4).map(drop),
		),
		(
			"list-sessions",
			Permission::ContextSessions,
			state.list_sessions().map(drop),
		),
		(
			"client-info",
			Permission::ContextBasic,
			state.client_info().map(drop),
		),
		(
			"voice-detail",
			Permission::ContextVoice,
			state.voice_detail().map(drop),
		),
		(
			"fetch-message",
			Permission::ContextTimeline,
			state.fetch_message(2, 1).map(drop),
		),
		(
			"message-meta",
			Permission::ContextTimeline,
			state.message_meta(message_ref).map(drop),
		),
		(
			"reaction-users",
			Permission::ContextTimeline,
			state.reaction_users(message_ref, emoji.clone(), 10).map(drop),
		),
		(
			"fetch-user",
			Permission::ContextMembers,
			state.fetch_user(4).map(drop),
		),
		(
			"attachment-bytes",
			Permission::ContextAttachments,
			state.attachment_bytes(message_ref, 6).map(drop),
		),
		(
			"avatar-bytes",
			Permission::ContextAttachments,
			state.avatar_bytes(4, 64).map(drop),
		),
		(
			"list-favorite-emojis",
			Permission::ContextExpressions,
			state.list_favorite_emojis().map(drop),
		),
		(
			"list-plugins",
			Permission::PluginsList,
			state.list_plugins().map(drop),
		),
		(
			"composer-get-draft",
			Permission::Composer,
			state.composer_get_draft().map(drop),
		),
		(
			"set-status",
			Permission::ActionsPresence,
			state.set_status("online".to_owned()),
		),
		(
			"set-custom-status",
			Permission::ActionsPresence,
			state.set_custom_status("probe".to_owned(), None, None),
		),
		("mark-read", Permission::ActionsAck, state.mark_read(2)),
		(
			"mark-unread",
			Permission::ActionsAck,
			state.mark_unread(message_ref),
		),
		(
			"mark-guild-read",
			Permission::ActionsAck,
			state.mark_guild_read(3),
		),
		(
			"start-reply",
			Permission::ActionsCompose,
			state.start_reply(message_ref, false),
		),
		(
			"start-edit",
			Permission::ActionsCompose,
			state.start_edit(message_ref),
		),
		(
			"jump-to-message",
			Permission::ActionsCompose,
			state.jump_to_message(message_ref),
		),
		("jump-to-top", Permission::ActionsCompose, state.jump_to_top(2)),
		(
			"edit-message",
			Permission::ActionsEdit,
			state.edit_message(message_ref, "probe".to_owned()),
		),
		(
			"delete-message",
			Permission::ActionsDelete,
			state.delete_message(message_ref),
		),
		(
			"react",
			Permission::ActionsReact,
			state.react(message_ref, emoji.clone(), false),
		),
		(
			"remove-reaction",
			Permission::ActionsReact,
			state.remove_reaction(message_ref, emoji.clone()),
		),
		(
			"pick-files",
			Permission::ActionsUpload,
			state.pick_files(false).map(drop),
		),
		(
			"composer-attach-files",
			Permission::ActionsUpload,
			state.composer_attach_files(Vec::new()),
		),
		(
			"send-message-files",
			Permission::ActionsUpload,
			state.send_message_files(outgoing.clone(), Vec::new()),
		),
		(
			"composer-set-draft",
			Permission::Composer,
			state.composer_set_draft("probe".to_owned()),
		),
		(
			"composer-attach-sticker",
			Permission::Composer,
			state.composer_attach_sticker(7),
		),
		(
			"save-attachment",
			Permission::ActionsSave,
			state.save_attachment(message_ref, 6),
		),
		("join-voice", Permission::ActionsVoice, state.join_voice(None, 2)),
		("leave-voice", Permission::ActionsVoice, state.leave_voice()),
		(
			"set-guild-notifications",
			Permission::ActionsSettings,
			state.set_guild_notifications(3, settings),
		),
		(
			"set-channel-notifications",
			Permission::ActionsSettings,
			state.set_channel_notifications(2, settings),
		),
		(
			"set-client-option",
			Permission::ActionsSettings,
			state.set_client_option(ClientOption::StreamerMode(false)),
		),
		(
			"set-dm-sections",
			Permission::ActionsSettings,
			state.set_dm_sections(Vec::<DmSection>::new()),
		),
		(
			"clone-emoji",
			Permission::ActionsGuild,
			state.clone_emoji(3, emoji.clone(), "probe".to_owned()),
		),
		(
			"clone-sticker",
			Permission::ActionsGuild,
			state.clone_sticker(3, 7),
		),
		(
			"register-keybind",
			Permission::UiKeybinds,
			state.register_keybind(keybind),
		),
		(
			"unregister-keybind",
			Permission::UiKeybinds,
			state.unregister_keybind("probe".to_owned()),
		),
		(
			"speak",
			Permission::ActionsSpeak,
			state.speak("probe".to_owned(), 1.0, 1.0),
		),
		(
			"open-settings",
			Permission::ActionsNavigate,
			state.open_settings("account".to_owned()),
		),
		(
			"navigate-back",
			Permission::ActionsNavigate,
			state.navigate_back(),
		),
		(
			"navigate-forward",
			Permission::ActionsNavigate,
			state.navigate_forward(),
		),
		(
			"set-menu-items-ext",
			Permission::UiMount,
			state.set_menu_items_ext("probe".to_owned(), Vec::new()),
		),
		(
			"watch-clicks",
			Permission::UiInput,
			state.watch_clicks(watch.clone()),
		),
		(
			"watch-channel-clicks",
			Permission::UiInput,
			state.watch_channel_clicks(watch),
		),
	];
	let total = results.len();
	let wrong: Vec<&str> = results
		.into_iter()
		.filter(|(_, expected, result)| !denied_matches(result.clone(), *expected))
		.map(|(name, _, _)| name)
		.collect();
	if wrong.is_empty() {
		recorder.pass(
			"ungranted host calls return denied",
			started.elapsed(),
			format!("{total} of {total} imports denied"),
		);
	} else {
		recorder.fail(
			"ungranted host calls return denied",
			started.elapsed(),
			format!("not denied: {}", wrong.join(", ")),
		);
	}

	let started = Instant::now();
	let granted = runtime.grants().update(GATE_PLUGIN, |grant| {
		*grant = PluginGrant::default();
		grant.permissions.insert(Permission::ActionsSend);
	});
	if granted.is_err() {
		recorder.fail(
			"send-message rate limit",
			started.elapsed(),
			"could not grant permission",
		);
		return recorder.checks;
	}
	let mut limited_at = None;
	for attempt in 1..=6 {
		state.begin_gesture();
		if matches!(state.send_message(outgoing.clone()), Err(HostError::RateLimited)) {
			limited_at = Some(attempt);
			break;
		}
	}
	match limited_at {
		Some(4) => recorder.pass(
			"send-message rate limit",
			started.elapsed(),
			"burst of 3 then rate-limited",
		),
		other => recorder.fail(
			"send-message rate limit",
			started.elapsed(),
			format!("first rate-limited call was {other:?}, expected 4"),
		),
	}
	recorder.checks
}

fn sample_message() -> MessageView {
	let now_ms = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.as_millis() as i64);
	MessageView {
		id: 1,
		channel_id: 2,
		guild_id: Some(3),
		author: User {
			id: 4,
			username: "conformance".to_owned(),
			display_name: "Conformance".to_owned(),
			avatar: None,
			bot: false,
		},
		content: "hello".to_owned(),
		timestamp_ms: now_ms - 60_000,
		edited: false,
		reply_to: None,
		flags: 0,
	}
}

fn sample_full(view: &MessageView) -> MessageFull {
	MessageFull {
		view: view.clone(),
		attachments: Vec::new(),
		embeds: Vec::new(),
		stickers: Vec::new(),
		reactions: Vec::new(),
		mentions: Vec::new(),
		mine: false,
		lifecycle: Lifecycle::Live,
	}
}

fn sample_meta() -> MessageMeta {
	MessageMeta {
		guild_id: Some(3),
		reply_to: None,
		flags: 0,
		latency_ms: None,
		author_color: None,
		pinned: false,
		edited_ms: None,
		nsfw_channel: false,
	}
}

fn sample_context() -> UiContext {
	UiContext {
		guild_id: Some(3),
		channel_id: Some(2),
		message_id: Some(1),
		user_id: Some(4),
		url: Some("https://example.com/".to_owned()),
	}
}

fn sample_channel() -> ChannelDetail {
	ChannelDetail {
		id: 2,
		guild_id: Some(3),
		name: "general".to_owned(),
		kind: 0,
		topic: None,
		nsfw: false,
		parent_id: None,
		position: 0,
		recipients: Vec::new(),
		owner_id: None,
		last_message_id: None,
		rate_limit: 0,
		created_ms: 0,
	}
}

fn hook2_probes(hook: Hook) -> Vec<(&'static str, CallKind)> {
	let message = sample_message();
	let content = md_to_rich(&[markdown::Node::Text("hello".to_owned())]);
	match hook {
		Hook::BeforeSend => vec![(
			"before-send-full",
			CallKind::BeforeSendFull(Arc::new(OutgoingFull {
				channel_id: 2,
				content: "hello".to_owned(),
				reply_to: None,
				mention_reply: false,
				silent: false,
				attachments: Vec::new(),
				stickers: Vec::new(),
			})),
		)],
		Hook::DisplayUser => vec![(
			"display-user-full",
			CallKind::DisplayUsersFull(Arc::from([(Some(3), message.author.clone(), None)])),
		)],
		Hook::Intercept => vec![
			(
				"intercept-message-ext",
				CallKind::InterceptExt {
					msg: Arc::new(sample_full(&message)),
					meta: Arc::new(sample_meta()),
					content: Arc::new(content.clone()),
				},
			),
			(
				"intercept-messages-ext",
				CallKind::InterceptBatchExt(Arc::from([(sample_full(&message), sample_meta(), content)])),
			),
		],
		Hook::DraftChange => vec![(
			"on-draft-change",
			CallKind::DraftChange {
				channel_id: 2,
				text: "hello".to_owned(),
			},
		)],
		Hook::AutocompleteEmoji => vec![(
			"autocomplete-emoji",
			CallKind::AutocompleteEmoji {
				query: "pr".to_owned(),
				candidates: Arc::from([EmojiInfo {
					emoji: EmojiRef {
						id: Some(5),
						name: "probe".to_owned(),
						animated: false,
					},
					guild_id: Some(3),
					usable: true,
					available: true,
				}]),
			},
		)],
		Hook::DecorateChannel => vec![(
			"decorate-channel",
			CallKind::DecorateChannels(Arc::from([sample_channel()])),
		)],
		Hook::Notification => vec![(
			"on-notification",
			CallKind::Notification(NotificationInfo {
				id: 1,
				channel_id: 2,
				guild_id: Some(3),
				message: None,
				author: Some(4),
				kind: NotificationKind::Message,
				mentions_me: false,
				mentions_everyone: false,
			}),
		)],
		Hook::MessageClick => vec![(
			"on-message-click",
			CallKind::MessageClick(MessageClick {
				message: MessageRef {
					id: 1,
					channel_id: 2,
					guild_id: Some(3),
				},
				kind: ClickKind::Single,
				modifiers: Modifiers::empty(),
				target: ClickTarget::Body,
			}),
		)],
		Hook::ChannelClick => vec![(
			"on-channel-click",
			CallKind::ChannelClick(ChannelClick {
				guild_id: Some(3),
				channel_id: 2,
				kind: ClickKind::Single,
				modifiers: Modifiers::empty(),
			}),
		)],
		Hook::LinkClick => vec![(
			"on-link-click",
			CallKind::LinkClick {
				url: "https://example.com/".to_owned(),
				context: sample_context(),
			},
		)],
	}
}

fn check_hooks2(
	manifest: &Manifest,
	instance: &PluginInstance,
	config: &RuntimeConfig,
	recorder: &mut Recorder,
) {
	for hook in Hook::ALL.into_iter().filter(|hook| manifest.hooks.contains(hook)) {
		for (name, kind) in hook2_probes(hook) {
			let deadline = kind.timeout(&config.timeouts);
			let started = Instant::now();
			let outcome = instance.call(kind).map(|_: Output| ());
			let elapsed = started.elapsed();
			match outcome {
				Ok(()) if within(elapsed, deadline, config) => recorder.pass(name, elapsed, "ok"),
				Ok(()) => recorder.fail(name, elapsed, "over deadline"),
				Err(error) => recorder.fail(name, elapsed, error.to_string()),
			}
		}
	}
}

fn check_plugin(runtime: &Runtime, dir: &Path) -> PluginReport {
	let mut recorder = Recorder::new();
	let manifest = match Package::from_dir(dir) {
		Ok(package) => package.manifest,
		Err(error) => {
			recorder.fail("package", Duration::ZERO, error.to_string());
			return PluginReport {
				id: dir.display().to_string(),
				checks: recorder.checks,
			};
		}
	};
	let id = manifest.id.clone();
	if manifest.entry.is_none() {
		recorder.pass("package", Duration::ZERO, "theme-only, no wasm entry");
		return PluginReport {
			id,
			checks: recorder.checks,
		};
	}
	if let Err(error) = runtime.grants().grant_manifest(&manifest) {
		recorder.fail("grant manifest permissions", Duration::ZERO, error.to_string());
		return PluginReport {
			id,
			checks: recorder.checks,
		};
	}
	let manager = PluginManager::new(runtime.clone());
	let started = Instant::now();
	if let Err(error) = manager.load_one(dir) {
		recorder.fail("compile and init", started.elapsed(), error.to_string());
		return PluginReport {
			id,
			checks: recorder.checks,
		};
	}
	recorder.pass("compile and init", started.elapsed(), "init returned ok");
	let Ok(instance) = manager.instance(&id) else {
		recorder.fail("instance", Duration::ZERO, "no running instance");
		return PluginReport {
			id,
			checks: recorder.checks,
		};
	};
	check_hooks(&manifest, &instance, runtime.config(), &mut recorder);
	check_hooks2(&manifest, &instance, runtime.config(), &mut recorder);
	check_guest_denial(&manifest, &instance, &mut recorder);
	check_guest_context_denial(&manifest, &instance, &mut recorder);
	check_interruption(&instance, runtime.config(), &mut recorder);
	check_memory_cap(&instance, runtime.config(), &mut recorder);
	manager.disable(&id).ok();
	PluginReport {
		id,
		checks: recorder.checks,
	}
}

fn within(elapsed: Duration, deadline: Duration, config: &RuntimeConfig) -> bool {
	elapsed <= deadline + config.caller_grace + DEADLINE_SLACK
}

fn check_hooks(
	manifest: &Manifest,
	instance: &PluginInstance,
	config: &RuntimeConfig,
	recorder: &mut Recorder,
) {
	let message = sample_message();
	let timeouts = &config.timeouts;

	if manifest.requests(Permission::MessageRender) {
		let started = Instant::now();
		let outcome = instance.render_message(
			Arc::new(message.clone()),
			Arc::new(md_to_tree(&[markdown::Node::Text("hello".to_owned())])),
		);
		let elapsed = started.elapsed();
		match outcome {
			Ok(Some(tree)) if crate::convert::tree_to_md(&tree).is_none() => {
				recorder.fail("render-message", elapsed, "returned an invalid tree");
			}
			Ok(tree) if within(elapsed, timeouts.render_message, config) => {
				let detail = match tree {
					Some(tree) => format!("returned {} nodes", tree.nodes.len()),
					None => "returned none".to_owned(),
				};
				recorder.pass("render-message", elapsed, detail);
			}
			Ok(_) => recorder.fail("render-message", elapsed, "over deadline"),
			Err(error) => recorder.fail("render-message", elapsed, error.to_string()),
		}
	} else {
		recorder.skip("render-message", "permission not requested");
	}

	if manifest.requests(Permission::MessageSend) {
		let started = Instant::now();
		let outgoing = OutgoingMessage {
			channel_id: 2,
			content: "hello".to_owned(),
			reply_to: None,
		};
		let outcome = instance.before_send(Arc::new(outgoing));
		record_simple(recorder, "before-send", started.elapsed(), outcome.map(|_| ()));
	}
	if manifest.requests(Permission::MessageIntercept) {
		let started = Instant::now();
		let outcome = instance.intercept(
			Arc::new(sample_full(&message)),
			Arc::new(md_to_rich(&[markdown::Node::Text("hello".to_owned())])),
		);
		record_simple(
			recorder,
			"intercept-message",
			started.elapsed(),
			outcome.map(|_| ()),
		);
	}
	if manifest.requests(Permission::MessageFilter) {
		let started = Instant::now();
		let outcome = instance.filter_message(Arc::new(message.clone()));
		record_simple(recorder, "filter-message", started.elapsed(), outcome.map(|_| ()));
	}
	if manifest.requests(Permission::UserDisplay) {
		let started = Instant::now();
		let outcome = instance.display_users(Arc::from([(Some(3), message.author.clone())]));
		record_simple(recorder, "display-user", started.elapsed(), outcome.map(|_| ()));
	}

	let started = Instant::now();
	match instance.settings_schema() {
		Ok(schema) if serde_json::from_str::<serde_json::Value>(&schema).is_ok() => {
			recorder.pass(
				"settings-schema",
				started.elapsed(),
				format!("{} bytes of json", schema.len()),
			);
		}
		Ok(_) => recorder.fail("settings-schema", started.elapsed(), "not valid json"),
		Err(error) => recorder.fail("settings-schema", started.elapsed(), error.to_string()),
	}
}

fn record_simple(recorder: &mut Recorder, name: &str, elapsed: Duration, outcome: Result<(), CallError>) {
	match outcome {
		Ok(()) => recorder.pass(name, elapsed, "ok"),
		Err(error) => recorder.fail(name, elapsed, error.to_string()),
	}
}

fn probe(key: &str) -> Event {
	Event::SettingChanged(SettingChange {
		key: key.to_owned(),
		value: String::new(),
	})
}

fn wait_for_restart(instance: &PluginInstance, recorder: &mut Recorder, name: &str) {
	let started = Instant::now();
	while started.elapsed() < RESTART_WAIT {
		if instance.settings_schema().is_ok() {
			recorder.pass(name, started.elapsed(), "instance answered after restart");
			return;
		}
		std::thread::sleep(Duration::from_millis(25));
	}
	recorder.fail(name, started.elapsed(), "instance did not come back");
}

fn check_interruption(instance: &PluginInstance, config: &RuntimeConfig, recorder: &mut Recorder) {
	let started = Instant::now();
	let outcome = instance.call_event(probe(PROBE_SPIN_KEY));
	let elapsed = started.elapsed();
	let deadline = config.timeouts.on_event;
	match outcome {
		Err(CallError::Timeout) if within(elapsed, deadline, config) => {
			recorder.pass(
				"infinite loop in on-event is interrupted",
				elapsed,
				format!(
					"interrupted after {:.0} ms, deadline {} ms",
					elapsed.as_secs_f64() * 1000.0,
					deadline.as_millis()
				),
			);
			wait_for_restart(instance, recorder, "restart after interrupt");
		}
		Err(error) => recorder.fail(
			"infinite loop in on-event is interrupted",
			elapsed,
			error.to_string(),
		),
		Ok(()) => recorder.skip(
			"infinite loop in on-event is interrupted",
			"plugin ignored the spin probe",
		),
	}
}

fn check_memory_cap(instance: &PluginInstance, config: &RuntimeConfig, recorder: &mut Recorder) {
	let started = Instant::now();
	let outcome = instance.call_event(probe(PROBE_ALLOC_KEY));
	let elapsed = started.elapsed();
	match outcome {
		Err(CallError::MemoryLimit) => {
			recorder.pass(
				"allocation beyond the memory cap traps",
				elapsed,
				format!(
					"trapped at the {} MB cap",
					config.memory_limit_bytes / (1024 * 1024)
				),
			);
			wait_for_restart(instance, recorder, "restart after memory trap");
		}
		Err(error) => recorder.fail(
			"allocation beyond the memory cap traps",
			elapsed,
			error.to_string(),
		),
		Ok(()) => recorder.skip(
			"allocation beyond the memory cap traps",
			"plugin ignored the alloc probe",
		),
	}
}

fn check_guest_denial(manifest: &Manifest, instance: &PluginInstance, recorder: &mut Recorder) {
	let name = "denied call from guest returns denied";
	if manifest.requests(Permission::ActionsSend) {
		recorder.skip(name, "plugin holds actions-send");
		return;
	}
	let started = Instant::now();
	let outcome = instance.command(PROBE_SEND_COMMAND.to_owned(), String::new(), 2);
	let elapsed = started.elapsed();
	match outcome {
		Ok(Err(message)) if message.contains("Denied") && message.contains("ActionsSend") => {
			recorder.pass(name, elapsed, format!("guest saw {message}"));
		}
		Ok(Err(message)) => recorder.fail(name, elapsed, format!("unexpected error {message}")),
		Ok(Ok(())) => recorder.skip(name, "plugin ignored the send probe"),
		Err(error) => recorder.fail(name, elapsed, error.to_string()),
	}
}

fn check_guest_context_denial(manifest: &Manifest, instance: &PluginInstance, recorder: &mut Recorder) {
	let name = "denied context calls from guest return denied";
	if manifest.requests(Permission::ContextDirectory) || manifest.requests(Permission::ContextVoice) {
		recorder.skip(name, "plugin holds context-directory or context-voice");
		return;
	}
	let started = Instant::now();
	let outcome = instance.command(PROBE_CONTEXT_COMMAND.to_owned(), String::new(), 2);
	let elapsed = started.elapsed();
	match outcome {
		Ok(Err(message)) => {
			let expected = [
				"list-guilds:HostError::Denied(Permission::ContextDirectory)",
				"list-channels:HostError::Denied(Permission::ContextDirectory)",
				"list-members:HostError::Denied(Permission::ContextDirectory)",
				"voice-state:HostError::Denied(Permission::ContextVoice)",
			];
			let missing: Vec<&str> = expected
				.into_iter()
				.filter(|entry| !message.contains(entry))
				.collect();
			if missing.is_empty() {
				recorder.pass(name, elapsed, "guest saw 4 of 4 imports denied");
			} else {
				recorder.fail(name, elapsed, format!("not denied: {}", missing.join(", ")));
			}
		}
		Ok(Ok(())) => recorder.skip(name, "plugin ignored the context probe"),
		Err(error) => recorder.fail(name, elapsed, error.to_string()),
	}
}
