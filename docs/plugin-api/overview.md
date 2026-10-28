# Plugin API overview

Plugins are WebAssembly components that implement the `oxidcord:plugin@1.2.0` WIT package (`crates/plugin-host/wit/oxidcord.wit`). The host runs each plugin on its own worker thread inside wasmtime. A plugin reaches the outside world only through host interfaces, and every function there is gated by a permission the user granted.

Related pages:

- `views.md`: view trees, mount points, context menus, view events, toasts and modals.
- `interception.md`: `intercept-message`, `before-send`, message lifecycle and ordering.
- `building.md`: manifest, packaging, signing, timeouts.
- `publishing.md`: catalogs.

## Versions and worlds

The package defines two worlds.

| World | Imports | Exports | Built with |
| --- | --- | --- | --- |
| `plugin` | `host` | `guest` | `export_plugin!(T)` |
| `plugin-ext` | `host`, `ui`, `client` | `guest`, `hooks`, `hooks2` | `export_plugin!(T, ext)` with the SDK feature `ext` |

The `types`, `host`, `guest` and `hooks` interfaces are unchanged from 1.0.0 and 1.1.0. The host loads every plugin with one linker. It looks up the `oxidcord:plugin/hooks` export at load time: a component that exports it runs as an extended plugin, any other component runs as a 1.0 plugin and the 1.1 hooks are never called for it. The 1.2 hooks live in a separate `hooks2` export, looked up on its own, so a 1.1 build keeps working. A component whose `hooks` or `hooks2` export is present but does not match the API is listed as incompatible and never runs as a downgraded plugin. Components built against `oxidcord:plugin@1.0.0` load unchanged, because import and export lookups match any 1.x version.

The host implements API 1.2.0. Manifests with `api = "^1.0"` and `api = "^1.1"` keep loading. A plugin that uses the 1.1 interfaces should declare `api = "^1.1"`, and one that uses 1.2 features `api = "^1.2"`, so that older hosts list it as incompatible instead of failing to instantiate it.

## Guest hooks

### `guest` (all plugins)

| Hook | Permission needed | Default deadline | Result |
| --- | --- | --- | --- |
| `init` | none | 5 s | `result<_, string>`, an error counts as a crash |
| `on-event` | per event, see below | 200 ms | fire and forget |
| `render-message` | `message-render` | 50 ms | `option<md-tree>`, the replacement AST |
| `before-send` | `message-send` | 500 ms | `pass`, `replace(outgoing-message)` or `cancel(reason)` |
| `filter-message` | `message-filter` | 50 ms | `false` hides the message |
| `display-user` | `user-display` | 50 ms | optional name, color and badge override |
| `on-ui-action` | none | 2 s | replacement `ui-tree` for the slot that owns the action id |
| `on-command` | `ui-slots` (to register) | 2 s | `result<_, string>` |
| `settings-schema` | none | 500 ms | JSON schema as a string |

### `hooks` (extended plugins)

| Hook | Called when | Default deadline | Result |
| --- | --- | --- | --- |
| `render-view` | A contextual mount point the plugin provides is shown | 200 ms | `option<view-tree>` |
| `on-view-event` | The user interacts with a keyed node in one of the plugin's views | 2 s | `view-reply` |
| `on-menu-item` | The user picks one of the plugin's context menu items | 2 s | nothing |
| `intercept-message` | A message is rendered, needs `message-intercept` | 50 ms | `message-patch` |
| `on-timer` | A timer set with `client.set-timer` fires | 200 ms | nothing |
| `on-emoji-pick` | The user picks an emoji the account cannot use, needs `composer` | 200 ms | `pick-verdict` |

### `hooks2` (extended plugins, API 1.2)

The host calls a `hooks2` hook only for plugins that export `hooks2` and list the matching name in the manifest `hooks` array. Hooks without a manifest name are gated by their permission alone. Every hook answers with its no-op value when the plugin does not export `hooks2`.

| Hook | Manifest name | Permission | Default deadline |
| --- | --- | --- | --- |
| `before-send-full` | `before-send` | `message-send` | 500 ms |
| `display-user-full` | `display-user` | `user-display` | 50 ms plus 5 ms per user, batches of 50 |
| `intercept-message-ext` | `intercept` | `message-intercept` | 50 ms |
| `intercept-messages-ext` | `intercept` | `message-intercept` | 50 ms per message, at most 500 ms |
| `on-client-event` | none | per event | 200 ms |
| `on-message-click` | `message-click` | `ui-input` | 300 ms |
| `on-channel-click` | `channel-click` | `ui-input` | 300 ms |
| `on-link-click` | `link-click` | `actions-open-url` | 300 ms |
| `on-draft-change` | `draft-change` | `composer-watch` | 100 ms |
| `autocomplete-emoji` | `autocomplete-emoji` | `ui-input` | 30 ms |
| `decorate-channel` | `decorate-channel` | `ui-input` | 50 ms plus 5 ms per channel |
| `on-keybind` | none | `ui-keybinds` | 2 s |
| `on-notification` | `notification` | `notifications-filter` | 250 ms |
| `on-badge-click` | none | `user-display` | 2 s |

The 1.2 `client` and `ui` functions are declared and permission checked, and the host answers `host-error::unavailable` until the features that back them ship.

Events: `message-created`, `message-updated`, `message-deleted`, `reaction-added`, `typing`, `channel-switched`, `setting-changed`, `connection`, `voice-changed`. Message, reaction and typing events need `context-timeline`, and events from direct messages also need `context-private`. `channel-switched` needs `context-basic`. `voice-changed` needs `context-voice`, and calls in direct messages also need `context-private`. `setting-changed` and `connection` are always delivered.

## Host imports

### `host` (all plugins)

| Function | Permission | Limits |
| --- | --- | --- |
| `log` | none | 2 KB per line, 100 lines per second after a burst of 200 |
| `now-ms` | none | |
| `storage-get`, `storage-set`, `storage-delete`, `storage-list` | `storage` | 5 MB per plugin, 1024 keys, keys up to 256 bytes |
| `fetch` | `net-fetch` | https and port 443 only, hosts must be in `network_hosts` and approved, 10 requests per minute with a burst of 20, 4 MB response, 10 s |
| `send-message` | `actions-send` | needs a user action, 3 sends per action, 1 per 2 s with a burst of 3, 4000 bytes |
| `add-reaction` | `actions-react` | 1 per second, burst 5 |
| `open-channel` | `actions-navigate` | |
| `clipboard-write` | `actions-clipboard` | 64 KB |
| `notify` | `actions-notify` | 1 per 5 s |
| `open-url` | `actions-open-url` | http or https, 1 per 10 s |
| `current-user`, `current-channel` | `context-basic` | direct message channels are hidden without `context-private` |
| `channel-messages` | `context-timeline` | at most 100 messages, direct messages dropped without `context-private` |
| `display-name` | `context-members` | |
| `list-guilds`, `list-members` | `context-directory` | `list-members` returns at most 1000 members |
| `list-channels` | `context-directory` | a `guild-id` of `none` lists direct message channels and also needs `context-private` |
| `voice-state` | `context-voice` | direct message calls hidden without `context-private` |
| `register-slot`, `register-command` | `ui-slots` | names are `[a-z0-9._-]` |

### `ui` (extended plugins)

| Function | Permission | Limits |
| --- | --- | --- |
| `mount`, `update`, `unmount` | `ui-mount` | 32 mounts per plugin, 20 updates per second after a burst of 60 |
| `provide`, `invalidate` | `ui-mount` | contextual points only |
| `set-menu-items` | `ui-mount` | 16 items per menu, one submenu level |
| `toast` | `ui-mount` | 1 per 5 s, unlimited while handling a user action, 1.5 to 15 s on screen |
| `open-modal` | `ui-mount` | only while handling a user action |

### `client` (extended plugins)

| Function | Permission | Limits |
| --- | --- | --- |
| `set-timer`, `clear-timer` | none | 64 timers per plugin, first delay at least 250 ms, repeat 0 or at least 1 s |
| `list-emojis`, `list-stickers` | `context-expressions` | |
| `emoji-url`, `sticker-url` | none | Discord CDN URLs, size clamped to 16 to 512 |
| `composer-insert` | `composer` | 2000 bytes, 1 per second after a burst of 5 |
| `get-message`, `channel-messages-full` | `context-timeline` | direct messages need `context-private`, at most 100 messages |
| `send-as-user` | `actions-send-user` | needs a user action, shares the `send-message` limits |

A call denied for a 1.0 permission returns `host-error::denied(permission)`. A call denied for a permission added in 1.1 returns `host-error::invalid("permission <name> is not granted")`, because the 1.0 `permission` enum cannot grow. Rate limited calls return `rate-limited`, calls the app cannot serve return `unavailable`.

## User actions

The host marks a call as a user action while it handles real user input: `on-ui-action`, `on-command`, `on-view-event` and `on-menu-item`. The mark is cleared when the hook returns. `send-message` and `send-as-user` work only inside a user action, at most three times per action, and return `host-error::invalid("requires a user action")` otherwise. This is a change from 1.0, where `send-message` could be called from any hook. Plugins that send from a button or a slash command keep working. Toasts are not rate limited inside a user action, and modals can only be opened inside one.

## Permissions

The consent dialog groups permissions by tier. Low tier permissions are collapsed into one expandable line, medium ones get a line each, and high and sensitive ones carry a warning.

| Permission | Tier | Unlocks |
| --- | --- | --- |
| `context-basic` | low | Current user, current channel, channel switch events |
| `context-expressions` | low | `list-emojis`, `list-stickers` |
| `storage` | low | The four `storage-*` functions |
| `ui-slots` | low | `register-slot` and `register-command` |
| `ui-mount` | low | The `ui` interface |
| `actions-navigate` | low | `open-channel` |
| `actions-notify` | low | `notify` |
| `channels-hidden` | low | Channels the account cannot open stay in the channel list, dimmed and locked |
| `typing-hidden` | low | Your typing indicator is not sent to other people while the plugin is enabled |
| `context-timeline` | medium | Message, reaction and typing events, `channel-messages`, `get-message`, `channel-messages-full` |
| `context-directory` | medium | `list-guilds`, `list-channels`, `list-members` |
| `context-members` | medium | `display-name` |
| `context-voice` | medium | `voice-state` and `voice-changed` events |
| `message-render` | medium | `render-message` |
| `message-intercept` | medium | `intercept-message` |
| `user-display` | medium | `display-user` |
| `composer` | medium | `composer-insert` and `on-emoji-pick` |
| `actions-react` | medium | `add-reaction` |
| `actions-clipboard` | medium | `clipboard-write` |
| `actions-open-url` | medium | `open-url` |
| `message-send` | high | `before-send`. A rewrite cannot change the channel or reply target |
| `message-filter` | high | `filter-message` |
| `actions-send` | high | `send-message`, marked as sent through the plugin |
| `actions-send-user` | high | `send-as-user`, sent without the plugin marker |
| `net-fetch` | high | `fetch` to approved hosts |
| `context-private` | sensitive | Direct message data in every context feature and in the render, filter and intercept hooks |
| `message-retain` | sensitive | Keeping deleted messages visible through `message-patch.retain` |

API 1.2 adds the permissions `context-relationships`, `context-presence`, `context-profiles`, `context-roles`, `context-read-state`, `context-attachments`, `context-sessions`, `composer-watch`, `actions-presence`, `actions-ack`, `actions-compose`, `actions-edit`, `actions-delete`, `actions-upload`, `actions-save`, `actions-voice`, `actions-settings`, `actions-guild`, `actions-speak`, `ui-keybinds`, `ui-input`, `notifications-filter` and `plugins-list`. Like the 1.1 ones they deny with `host-error::invalid("permission <name> is not granted")`. Their per-function reference ships with the 1.2 docs.

`network_hosts` may not name `discord.com`, `discordapp.com`, `discord.gg`, `discord.media`, `discordapp.net` or any of their subdomains.

## Images

Every image a plugin shows (view images, embed cards, inline images in message content) must be an `https` URL. Images on `cdn.discordapp.com` and `media.discordapp.net` are always allowed. Images from other hosts are allowed in 1.0 `ui-tree` slots as before, and in 1.1 views and patches only when the host is listed in the manifest `image_hosts` and the user approved it. Images that fail the check are dropped from the tree.

## Trees instead of recursive nodes

WIT has no recursive types, so markdown, UI and view nodes are flat. A tree is a `nodes` list plus `roots`, and containers refer to their children by index. A child must have a higher index than its parent. Plugin data is untrusted: the host checks every node and drops bad nodes one at a time (out of range or backward indices, unknown color tokens, bad URLs, invalid icon names, too deep or too large trees) instead of rejecting the whole tree. Strings are clipped to their limits. The SDK hides the flat form: `Md` and `view::Node` are ordinary recursive Rust types.

## Sandbox

- One wasmtime instance per plugin on its own thread, with epoch interruption ticking every 10 ms.
- Linear memory is capped at 64 MB and tables at 10,000 elements. Exceeding the memory cap traps the plugin.
- WASI preview 2 is linked with an empty context: no preopened directories, environment or arguments, and TCP, UDP and name lookup are disabled.
- A trap, timeout or memory trap restarts the instance (new store, `init` runs again, timers cleared) after a backoff that starts at 100 ms and doubles up to 5 s. Five crashes within 60 seconds disable the plugin until it is enabled again. Plugin storage survives restarts.
- Each plugin has a queue. Events and timer firings are dropped oldest first beyond 256 queued items, and a repeating timer that is still queued is not queued again. Interactive calls jump ahead of queued events.
- All timers of all plugins share one thread that sleeps until the next deadline.

## Writing a plugin

A 1.0 style plugin implements `Plugin` and calls `export_plugin!(T)`. An extended plugin enables the `ext` feature of the SDK, implements `Plugin` and `PluginExt`, and calls `export_plugin!(T, ext)`. Every method of both traits has a no-op default.

```toml
[dependencies]
oxidcord-plugin-sdk = { version = "0.5", features = ["ext"] }
```

```rust
use oxidcord_plugin_sdk::model::{Space, Tone, ViewEvent};
use oxidcord_plugin_sdk::view::{self, Node};
use oxidcord_plugin_sdk::{Plugin, PluginExt, Reply, export_plugin, log, mount};

#[derive(Default)]
struct Counter {
	clicks: u32,
}

impl Counter {
	fn page(&self) -> Vec<Node> {
		vec![
			view::column()
				.gap(Space::Md)
				.padding(Space::Xl)
				.child(view::heading("Counter"))
				.child(view::text(format!("{} clicks", self.clicks)).muted())
				.child(view::button("add", "Add one").icon("plus").tone(Tone::Brand)),
		]
	}
}

impl Plugin for Counter {
	fn init(&mut self) -> Result<(), String> {
		let options = mount::Options::new("Counter").icon("calculator");
		let mounted = mount::mount(mount::APP_NAV, "counter", Vec::<Node>::new(), options.clone())
			.and_then(|()| mount::mount(mount::MAIN_VIEW, "counter", self.page(), options));
		if let Err(error) = mounted {
			log::warn!("could not mount the counter: {error:?}");
		}
		Ok(())
	}
}

impl PluginExt for Counter {
	fn on_view_event(&mut self, event: ViewEvent) -> Reply {
		if event.node == "add" {
			self.clicks += 1;
			return Reply::none().replace(self.page());
		}
		Reply::none()
	}
}

export_plugin!(Counter, ext);
```

The manifest of this plugin asks for `ui-mount`. See `plugins/notes-panel` and `plugins/fake-nitro` for complete extended plugins, `plugins/hello-timestamps` for a 1.0 plugin and `plugins/conformance-probe` for the misbehaving one the conformance runner uses.

## Conformance

`plugin_host::conformance::run(plugins_dir)` loads every plugin in a directory with its manifest permissions granted, then checks:

- ungranted calls to every gated 1.0 host function return `denied`, and `send-message` is rate limited after a burst of three;
- `init` succeeds and each hook the manifest has a permission for answers within its deadline, including `intercept-message` for plugins that ask for `message-intercept`;
- `settings-schema` returns valid JSON;
- a `setting-changed` event with key `oxidcord.conformance.spin` that makes the plugin loop forever is interrupted within the `on-event` deadline and the instance restarts;
- a `setting-changed` event with key `oxidcord.conformance.alloc` that makes the plugin allocate past the memory cap traps and the instance restarts.

Well behaved plugins ignore the two probe keys and those checks are reported as skipped.
