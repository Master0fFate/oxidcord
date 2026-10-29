# Plugin API overview

Plugins are WebAssembly components that implement the `oxidcord:plugin@1.2.0` WIT package (`crates/plugin-host/wit/oxidcord.wit`). The host runs each plugin on its own worker thread inside wasmtime. A plugin reaches the outside world only through host interfaces, and every function there is gated by a permission the user granted.

Related pages:

- `data.md`: every `client` read, the records it returns, cache and load behaviour.
- `actions.md`: every `client` write, user action rules, client options, keybinds, open-url schemes.
- `events.md`: `event` and `client-event` variants, the notification hook pipeline.
- `views.md`: view trees, mount points, context menus, view events, toasts and modals, display overrides.
- `interception.md`: `intercept-message`, `before-send`, message lifecycle and ordering.
- `building.md`: manifest, packaging, signing, timeouts.
- `publishing.md`: catalogs.
- `porting-vencord.md`: status of every Vencord plugin.

## Versions and worlds

The package defines two worlds.

| World | Imports | Exports | Built with |
| --- | --- | --- | --- |
| `plugin` | `host` | `guest` | `export_plugin!(T)` |
| `plugin-ext` | `host`, `ui`, `client` | `guest`, `hooks`, `hooks2` | `export_plugin!(T, ext)` with the SDK feature `ext` |

| Interface | Introduced | Status |
| --- | --- | --- |
| `types`, `host`, `guest` | 1.0 | Frozen |
| `ui`, `client`, `model`, `hooks` | 1.1 | Additive only: new functions, records, enums and variants. Existing items never change |
| `hooks2` | 1.2 | Frozen once released. New hooks go into a future `hooks3` |

### Compatibility rule

- The host loads every plugin with one linker. Import and export lookups match any 1.x version of the package, so components built against 1.0.0 or 1.1.0 load unchanged on a 1.2 host.
- A component that exports `oxidcord:plugin/hooks` runs as an extended plugin. Any other component runs as a 1.0 plugin and the 1.1 hooks are never called for it.
- `hooks2` is looked up on its own. A plugin may export `hooks` without `hooks2` (a 1.1 build) and keeps working; the 1.2 hooks then answer with their no-op value.
- A component whose `hooks` or `hooks2` export is present but does not match the API is listed as incompatible (`Error::Incompatible`) and never runs as a downgraded plugin.
- The host implements API 1.2.0. Manifests with `api = "^1.0"`, `api = "^1.1"` and `api = "^1.2"` load. Declare the lowest version that covers the functions the plugin uses, so older hosts list it as incompatible instead of failing to instantiate it: `^1.1` for the `ui`, `client` or `hooks` interfaces, `^1.2` for `hooks2` and any function added in 1.2.

## Guest hooks

### `guest` (all plugins)

| Hook | Permission needed | Default deadline | Result |
| --- | --- | --- | --- |
| `init` | none | 5 s | `result<_, string>`, an error counts as a crash |
| `on-event` | per event, see `events.md` | 200 ms | fire and forget |
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

The host calls a `hooks2` hook only for plugins that export `hooks2` and hold the permission in the table. Hooks with a manifest name are also called only when the manifest `hooks` array lists that name (see `building.md`); the consent dialog shows the list. The three hooks without a manifest name (`on-client-event`, `on-keybind`, `on-badge-click`) are gated by their permission alone. A plugin that does not export `hooks2` gets the no-op answer for every hook.

| Hook | Manifest name | Permission | Default deadline | Result |
| --- | --- | --- | --- | --- |
| `before-send-full` | `before-send` | `message-send` | 500 ms | `send-decision-full` |
| `display-user-full` | `display-user` | `user-display` | 50 ms plus 5 ms per user, batches of 50 | `option<display-override-full>` |
| `intercept-message-ext` | `intercept` | `message-intercept` | 50 ms | `message-patch-ext` |
| `intercept-messages-ext` | `intercept` | `message-intercept` | 50 ms per message, at most 500 ms | `list<message-patch-ext>`, one per input |
| `on-client-event` | none | per event | 200 ms | fire and forget |
| `on-message-click` | `message-click` | `ui-input` | 300 ms | fire and forget, user action |
| `on-channel-click` | `channel-click` | `ui-input` | 300 ms | fire and forget, user action |
| `on-link-click` | `link-click` | `actions-open-url` | 300 ms, shared by all plugins | `link-verdict`, user action |
| `on-draft-change` | `draft-change` | `composer-watch` | 100 ms | fire and forget |
| `autocomplete-emoji` | `autocomplete-emoji` | `ui-input` | 30 ms, shared by all plugins | `list<u32>` |
| `decorate-channel` | `decorate-channel` | `ui-input` | 50 ms plus 5 ms per channel | `list<badge>` |
| `on-keybind` | none | `ui-keybinds` | 2 s | fire and forget, user action |
| `on-notification` | `notification` | `notifications-filter` | 250 ms, shared by all plugins | `notification-verdict` |
| `on-badge-click` | none | `user-display` | 2 s | fire and forget, user action |

Replacement rules:

- A plugin that declares `before-send` gets `before-send-full` instead of `before-send`; `display-user` replaces `display-user` and `intercept` replaces `intercept-message` the same way. A plugin that does not declare the name keeps the older hook.
- `intercept-messages-ext` is the batch form the render worker uses for bulk renders (up to 16 messages per call while an extended plugin is active). A single visible message uses `intercept-message-ext`. The default SDK implementation of the batch calls `intercept-message-ext` for each item.
- `autocomplete-emoji` answers with indices into the candidate list (at most 200 candidates). The composer shows the listed candidates in that order and hides the ones that are not listed. An empty answer leaves the default suggestions. Answers are chained in plugin order: each plugin receives the candidates as the previous plugin ordered and filtered them, and its indices refer to that list, so a filter and a reorder from different plugins combine. Typing is debounced by 100 ms; the answer is only applied when it arrives within 300 ms of the request and the first suggestion is still selected.
- `on-link-click`: `pass` leaves the link alone, `consume` stops the open, `open(url)` opens another URL through the open-url validator (http, https or an approved app scheme, 2048 bytes). The first plugin that does not pass decides. The UI defers the open asynchronously and never blocks on the hooks; if they take longer than 300 ms the original link opens.
- `on-draft-change` is coalesced: only the latest text per channel stays queued.
- `decorate-channel` runs in the background for the visible channels (the selected server's channels and up to 100 DMs, at most 300 channels), is cached, and runs again after `invalidate-channel` or a `channel-list-changed` event. Direct messages are only passed to plugins with `context-private`.
- Circuit breaker: three timeouts in a row of `intercept`, `before-send`, `display-user`, `notification`, `link-click`, `autocomplete-emoji` or `decorate-channel` disable that hook for the plugin until it is reloaded. A success resets the count.

## Host imports

### `host` (all plugins)

| Function | Permission | Limits |
| --- | --- | --- |
| `log` | none | 2 KB per line, 100 lines per second after a burst of 200 |
| `now-ms` | none | |
| `storage-get`, `storage-set`, `storage-delete`, `storage-list` | `storage` | 5 MB per plugin, 1024 keys, keys up to 256 bytes |
| `fetch` | `net-fetch` | https and port 443 only, hosts must be in `network_hosts` and approved, 10 requests per minute with a burst of 20, 8 MB response, 10 s. Only in `init`, `on-event`, `on-timer` and user action hooks |
| `send-message` | `actions-send` | needs a user action, 3 sends per action, 1 per 2 s with a burst of 3, 4000 bytes |
| `add-reaction` | `actions-react` | 1 per second, burst 5 |
| `open-channel` | `actions-navigate` | |
| `clipboard-write` | `actions-clipboard` | 64 KB |
| `notify` | `actions-notify` | 1 per 5 s |
| `open-url` | `actions-open-url` | http or https, 1 per 10 s. App schemes need `url_schemes` and a user action, see `actions.md` |
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
| `set-menu-items`, `set-menu-items-ext` | `ui-mount` | 16 items per menu, one submenu level |
| `context-detail` | `ui-mount` | only inside `render-view`, `on-view-event` and `on-menu-item` |
| `toast` | `ui-mount` | 1 per 5 s, unlimited while handling a user action, 1.5 to 15 s on screen |
| `open-modal` | `ui-mount` | only while handling a user action |
| `watch-clicks`, `watch-channel-clicks` | `ui-input` | 4 watches per plugin, shares the 20 per second `ui` limit. See `views.md` |

Mount points, menus and display overrides are described in `views.md`.

### `client` (extended plugins)

| Function | Permission | Limits |
| --- | --- | --- |
| `set-timer`, `clear-timer` | none | 64 timers per plugin, first delay at least 250 ms, repeat 0 or at least 1 s |
| `list-emojis`, `list-stickers` | `context-expressions` | |
| `emoji-url`, `sticker-url` | none | Discord CDN URLs, size clamped to 16 to 512 |
| `composer-insert` | `composer` | 2000 bytes, 1 per second after a burst of 5 |
| `get-message`, `channel-messages-full` | `context-timeline` | direct messages need `context-private`, at most 100 messages |
| `send-as-user` | `actions-send-user` | needs a user action, shares the `send-message` limits |
| 1.2 reads | per function | see `data.md` |
| 1.2 writes | per function | see `actions.md` |

A call denied for a 1.0 permission returns `host-error::denied(permission)`. A call denied for a permission added in 1.1 or 1.2 returns `host-error::invalid("permission <name> is not granted")`, because the 1.0 `permission` enum cannot grow. Rate limited calls return `rate-limited`, calls the app cannot serve return `unavailable`.

## Call contexts

The host records the kind of the hook that is currently running. It decides whether a function that loads data over the network may do so, whether `fetch` is available, and whether a function that needs a user action may run.

| Context | Hooks | Loads | `fetch` |
| --- | --- | --- | --- |
| Init | `init` | cache only | yes |
| Event | `on-event` (not `setting-changed`), `on-client-event`, `on-draft-change` | yes | yes |
| Timer | `on-timer` | yes | yes |
| User action | `on-ui-action`, `on-command`, `on-view-event`, `on-menu-item`, `on-keybind`, `on-badge-click`, `on-message-click`, `on-channel-click`, `on-link-click` | yes | yes |
| Render | `render-message`, `filter-message`, `render-view` | cache only | `unavailable` |
| Intercept | `intercept-message`, `intercept-message-ext`, `intercept-messages-ext`, `before-send`, `before-send-full`, `on-emoji-pick`, `setting-changed`, `settings-schema` | cache only | `unavailable` |
| Display | `display-user`, `display-user-full` | cache only | `unavailable` |
| Decorate | `decorate-channel` | cache only | `unavailable` |
| Autocomplete | `autocomplete-emoji` | cache only | `unavailable` |
| Notification | `on-notification` | cache only | `unavailable` |

The functions that can load are `user-profile`, `fetch-message`, `reaction-users`, `fetch-user`, `attachment-bytes` and `avatar-bytes`. In a cache-only context they return the cached value or `unavailable`. In an Event, Timer or user action context a miss triggers one REST load through the session, deduplicated across plugins and cached in the client state, and the plugin thread waits for it for up to 5 seconds. Time spent waiting in a load, a `fetch` or a native file dialog does not count against the hook deadline.

## User actions

The host marks a call as a user action while it handles real user input: `on-ui-action`, `on-command`, `on-view-event`, `on-menu-item`, `on-keybind`, `on-badge-click`, `on-message-click`, `on-channel-click` and `on-link-click`. The mark is cleared when the hook returns.

- `send-message`, `send-as-user` and `send-message-files` work only inside a user action, at most three sends per action, and return `host-error::invalid("requires a user action")` otherwise.
- Every function marked "user action" in `actions.md` returns the same error outside one.
- Toasts are not rate limited inside a user action, and modals can only be opened inside one.

## Permissions

The consent dialog groups permissions by tier. Low tier permissions are collapsed into one expandable line, medium ones get a line each, and high and sensitive ones carry a warning.

| Permission | Tier | Since | Unlocks |
| --- | --- | --- | --- |
| `context-basic` | low | 1.0 | Current user, current channel, channel switch events, `client-info` |
| `context-expressions` | low | 1.1 | `list-emojis`, `list-stickers`, `list-favorite-emojis` |
| `storage` | low | 1.0 | The four `storage-*` functions |
| `ui-slots` | low | 1.0 | `register-slot` and `register-command` |
| `ui-mount` | low | 1.1 | The `ui` interface: mount points, menus, `context-detail`, toasts, modals |
| `actions-navigate` | low | 1.0 | `open-channel`, `open-settings`, `navigate-back`, `navigate-forward` |
| `actions-notify` | low | 1.0 | `notify` |
| `channels-hidden` | low | 1.1 | Channels the account cannot open stay in the channel list, dimmed and locked |
| `typing-hidden` | low | 1.1 | Your typing indicator is not sent to other people while the plugin is enabled |
| `actions-compose` | low | 1.2 | `start-reply`, `start-edit`, `jump-to-message`, `jump-to-top` |
| `actions-speak` | low | 1.2 | `speak` |
| `ui-keybinds` | low | 1.2 | `register-keybind`, `unregister-keybind`, `on-keybind` |
| `ui-input` | low | 1.2 | `watch-clicks`, `watch-channel-clicks`, `on-message-click`, `on-channel-click`, `autocomplete-emoji`, `decorate-channel` |
| `context-timeline` | medium | 1.0 | Message, reaction and typing events, `channel-messages`, `get-message`, `channel-messages-full`, `fetch-message`, `message-meta`, `reaction-users`, `reaction-removed` and `message-acked` events |
| `context-directory` | medium | 1.0 | `list-guilds`, `list-channels`, `list-members`, `guild-detail`, `channel-detail`, `list-folders`, guild and channel list events |
| `context-members` | medium | 1.0 | `display-name`, `fetch-user` |
| `context-voice` | medium | 1.0 | `voice-state`, `voice-detail` and `voice-changed` events |
| `message-render` | medium | 1.0 | `render-message` |
| `message-intercept` | medium | 1.1 | `intercept-message` and `intercept-message-ext` |
| `user-display` | medium | 1.0 | `display-user`, `display-user-full`, `on-badge-click` |
| `composer` | medium | 1.1 | `composer-insert`, `composer-get-draft`, `composer-set-draft`, `composer-attach-sticker`, `on-emoji-pick` |
| `actions-react` | medium | 1.0 | `add-reaction`, `react`, `remove-reaction` |
| `actions-clipboard` | medium | 1.0 | `clipboard-write` |
| `actions-open-url` | medium | 1.0 | `open-url`, `on-link-click`, app schemes from `url_schemes` |
| `context-relationships` | medium | 1.2 | `list-relationships`, relationship events, friend nicknames |
| `context-presence` | medium | 1.2 | `user-presence`, `idle-seconds`, `presence-changed` and `idle-changed` events, presence in `display-user-full` |
| `context-profiles` | medium | 1.2 | `user-profile` |
| `context-roles` | medium | 1.2 | `list-roles`, `member-info`, `channel-permissions` |
| `context-read-state` | medium | 1.2 | `read-state`, `unread-channels`, `channel-muted`, `unread-changed` events |
| `composer-watch` | medium | 1.2 | `on-draft-change` |
| `actions-presence` | medium | 1.2 | `set-status`, `set-custom-status` |
| `actions-ack` | medium | 1.2 | `mark-read`, `mark-unread`, `mark-guild-read` |
| `actions-save` | medium | 1.2 | `save-attachment` |
| `actions-settings` | medium | 1.2 | `set-guild-notifications`, `set-channel-notifications`, `set-client-option`, `set-dm-sections` |
| `notifications-filter` | medium | 1.2 | `on-notification` |
| `plugins-list` | medium | 1.2 | `list-plugins` |
| `message-send` | high | 1.0 | `before-send` and `before-send-full`. A rewrite cannot change the channel or reply target. Also sees messages typed in direct messages, see below |
| `message-filter` | high | 1.0 | `filter-message` |
| `actions-send` | high | 1.0 | `send-message`, marked as sent through the plugin |
| `actions-send-user` | high | 1.1 | `send-as-user`, sent without the plugin marker |
| `net-fetch` | high | 1.0 | `fetch` to approved hosts |
| `context-attachments` | high | 1.2 | `attachment-bytes`, `avatar-bytes` |
| `actions-edit` | high | 1.2 | `edit-message` |
| `actions-delete` | high | 1.2 | `delete-message` |
| `actions-upload` | high | 1.2 | `pick-files`, `composer-attach-files`, `send-message-files` |
| `actions-voice` | high | 1.2 | `join-voice`, `leave-voice` |
| `actions-guild` | high | 1.2 | `clone-emoji`, `clone-sticker` |
| `context-private` | sensitive | 1.0 | Direct message data, see the next section |
| `message-retain` | sensitive | 1.1 | Keeping deleted messages visible through `message-patch.retain` |
| `context-sessions` | sensitive | 1.2 | `list-sessions` |

`network_hosts` may not name `discord.com`, `discordapp.com`, `discord.gg`, `discord.media`, `discordapp.net` or any of their subdomains. A plugin never receives the token and cannot send raw Discord REST requests; every write maps to a typed session command and the session applies its own permission checks.

### Direct messages

Direct messages and group DMs are private by default. A plugin without `context-private` does not see them anywhere:

- Reads and context features return `none`, an empty list, `false` or `unavailable` for a DM channel, message or call: `current-channel`, `channel-messages`, `list-channels`, `voice-state`, `voice-detail`, `get-message`, `fetch-message`, `message-meta`, `reaction-users`, `attachment-bytes`, `read-state`, `unread-channels`, `channel-muted`, `channel-permissions`, `channel-detail`, `composer-get-draft`.
- Events from DMs are not delivered: message, reaction and typing events (typing in a DM is gated on the channel, not on the guild), `voice-changed`, `dm-closed`, and `unread-changed`, `reaction-removed` and `message-acked` for DM messages.
- Hooks do not run for DM content: `render-message`, `filter-message`, `intercept-message*`, `on-message-click`, `on-channel-click`, `on-link-click` from a DM, `on-draft-change`, `decorate-channel` and `on-notification` (DM and group DM notifications are not delivered).
- Views and menus: contextual `render-view` providers are skipped for DM contexts, `on-menu-item` is not called for a DM target, menu groups of plugins without `context-private` are dropped from DM rows, and `context-detail` has `selection`, `image-url` and `attachment-id` cleared.
- Actions on a DM channel (reply, edit, delete, react, mark read, jump, save, attach) fail for plugins without `context-private`. A channel the client does not know counts as a guild channel and is refused.

`message-send` is the exception: `before-send` and `before-send-full` see the outgoing message wherever the user types it, including DMs, because it is the user's own text and the permission is high tier. The consent text for `message-send` says so.

## Images

Every image a plugin shows (view images, embed cards, inline images in message content, badge images, avatar, banner and decoration overrides, attachment URL swaps) must be an `https` URL. Images on `cdn.discordapp.com` and `media.discordapp.net` are always allowed. Images from other hosts are allowed in 1.0 `ui-tree` slots as before, and in 1.1 and 1.2 views, patches and overrides only when the host is listed in the manifest `image_hosts` and the user approved it. This is the plugin's `ImagePolicy`, computed from the grant and rebuilt when the grant changes. Images that fail the check are dropped from the tree. An attachment `url` override must stay on the original attachment's host.

## Trees instead of recursive nodes

WIT has no recursive types, so markdown, UI and view nodes are flat. A tree is a `nodes` list plus `roots`, and containers refer to their children by index. A child must have a higher index than its parent. Plugin data is untrusted: the host checks every node and drops bad nodes one at a time (out of range or backward indices, unknown color tokens, bad URLs, invalid icon names, too deep or too large trees) instead of rejecting the whole tree. Strings are clipped to their limits. The SDK hides the flat form: `Md` and `view::Node` are ordinary recursive Rust types.

## Sandbox

- One wasmtime instance per plugin on its own thread, with epoch interruption ticking every 10 ms.
- Linear memory is capped at 64 MB and tables at 10,000 elements. Exceeding the memory cap traps the plugin.
- WASI preview 2 is linked with an empty context: no preopened directories, environment or arguments, and TCP, UDP and name lookup are disabled.
- A trap, timeout or memory trap restarts the instance (new store, `init` runs again, timers cleared) after a backoff that starts at 100 ms and doubles up to 5 s. Five crashes within 60 seconds disable the plugin until it is enabled again. Plugin storage survives restarts.
- Each plugin has a queue. Events and timer firings are dropped oldest first beyond 256 queued items, and a repeating timer that is still queued is not queued again. Interactive calls jump ahead of queued events and background batches.
- All timers of all plugins share one thread that sleeps until the next deadline.
- Keybinds, click watches, client options and DM sections registered by a plugin are removed when it is disabled or uninstalled.

## Writing a plugin

A 1.0 style plugin implements `Plugin` and calls `export_plugin!(T)`. An extended plugin enables the `ext` feature of the SDK (version 0.5, see `crates/plugin-sdk/Cargo.toml`), implements `Plugin` and `PluginExt`, and calls `export_plugin!(T, ext)`, which exports `hooks` and `hooks2`. Every method of both traits has a no-op default. The SDK modules are `data` (1.2 reads), `actions` (1.2 writes), `client` (1.1 functions), `mount`, `menu`, `view`, `patch`, `timer`, `storage` and `log`; see `building.md`.

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

The manifest of this plugin asks for `ui-mount`. See `plugins/notes-panel` and `plugins/fake-nitro` for complete extended plugins, `plugins/hello-timestamps` for a 1.0 plugin, `plugins/README.md` for the first-party plugins and `plugins/conformance-probe` for the misbehaving one the conformance runner uses.

## Conformance

`plugin_host::conformance::run(plugins_dir)` first runs a `host` row, then loads every plugin in the directory with its manifest permissions granted. The host row checks:

- every gated `host`, `ui` and `client` function (82 imports) denies an ungranted call. 1.0 permissions return `denied(permission)`, later ones return `invalid("permission <name> is not granted")`;
- `send-message` is rate limited after a burst of three;
- `before-send-full` replacements keep the channel, reply target and attachment set, sanitize attachment names and cap stickers at three;
- notification verdicts: a timeout counts as allow, an empty title is dropped, title and body are clipped to 100 and 500 bytes.

Each plugin row checks:

- `init` succeeds and each hook the manifest has a permission for answers within its deadline, including `intercept-message` for plugins that ask for `message-intercept`;
- every `hooks2` hook the manifest declares in `hooks` answers a probe within its deadline;
- `settings-schema` returns valid JSON;
- a plugin without `actions-send` sees `send-message` denied from inside the guest, and a plugin without `context-directory` or `context-voice` sees those imports denied;
- a `setting-changed` event with key `oxidcord.conformance.spin` that makes the plugin loop forever is interrupted within the `on-event` deadline and the instance restarts;
- a `setting-changed` event with key `oxidcord.conformance.alloc` that makes the plugin allocate past the memory cap traps and the instance restarts.

Well behaved plugins ignore the two probe keys and those checks are reported as skipped. `cargo xtask plugin conformance` prints one row per plugin plus the host row.

## Known gaps

| Area | Behaviour |
| --- | --- |
| `guild-detail.presence-count` | Always `none` |
| `list-favorite-emojis` | Always empty, the favourite emoji setting is not decoded |
| `client-event.guild-removed` | Never fires. Every removal of a server, including being kicked or banned, arrives as `guild-left` |
| `client-event.ready` | Sent when the gateway finishes its sync and the plugins have started. Plugins installed or enabled later do not get it |
| `client-event.presence-changed` | Only for the current user and friends |
| `speak` | Windows only (SAPI). `unavailable` on macOS and Linux |
| `idle-seconds`, `idle-changed` | Windows and macOS. `unavailable` on Linux, and the event never fires there |
| `user-profile.connections` | `id` is empty and `url` is `none` |
| `list-relationships` | `implicit` entries are users who share two or more loaded servers with you, capped at 1000 |
| `list-sessions` | The first call returns `unavailable` and queues a load; the list is available on a later call, and the client retries a failed load after 60 s |
| `message-meta.latency-ms` | Only for messages this client sent |
| Display overrides | One override per field: the first plugin in priority order that sets a field wins it, badges merge |
