# Plugin API overview

Plugins are WebAssembly components that implement the `oxidcord:plugin@1.0.0` WIT package (`crates/plugin-host/wit/oxidcord.wit`). The host runs each plugin on its own worker thread inside wasmtime. A plugin can only reach the outside world through the `host` interface, and every function there is gated by a permission the user granted.

## Shape of the API

The world is `plugin`: it imports the `host` interface and exports the `guest` interface. Shared data types live in the `types` interface.

### Guest exports (the hooks a plugin implements)

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

Events: `message-created`, `message-updated`, `message-deleted`, `reaction-added`, `typing`, `channel-switched`, `setting-changed`, `connection`, `voice-changed`. Message, reaction and typing events need `context-timeline`, and events from direct messages additionally need `context-private`. `channel-switched` needs `context-basic`. `voice-changed` carries a `voice-info` and needs `context-voice`, and calls in direct messages additionally need `context-private`. `setting-changed` and `connection` are always delivered.

### Host imports

| Function | Permission | Limits |
| --- | --- | --- |
| `log` | none | 2 KB per line, 100 lines per second after a burst of 200 |
| `now-ms` | none | |
| `storage-get`, `storage-set`, `storage-delete`, `storage-list` | `storage` | 5 MB per plugin, 1024 keys, keys up to 256 bytes |
| `fetch` | `net-fetch` | https and port 443 only, hosts must be in `network_hosts` and approved, 10 requests per minute with a burst of 20, 4 MB response, 10 s |
| `send-message` | `actions-send` | 1 per 2 s, burst 3, 4000 bytes |
| `add-reaction` | `actions-react` | 1 per second, burst 5 |
| `open-channel` | `actions-navigate` | |
| `clipboard-write` | `actions-clipboard` | 64 KB |
| `notify` | `actions-notify` | 1 per 5 s |
| `open-url` | `actions-open-url` | http or https, 1 per 10 s |
| `current-user`, `current-channel` | `context-basic` | direct message channels are hidden without `context-private` |
| `channel-messages` | `context-timeline` | at most 100 messages, direct messages dropped without `context-private` |
| `display-name` | `context-members` | |
| `list-guilds`, `list-members` | `context-directory` | `list-members` returns at most 1000 members, `limit` is clamped to 1 to 1000 |
| `list-channels` | `context-directory` | a `guild-id` of `none` lists direct message channels and also needs `context-private` |
| `voice-state` | `context-voice` | `none` when not in a call, direct message calls hidden without `context-private` |
| `register-slot`, `register-command` | `ui-slots` | slot names and command names are `[a-z0-9._-]` |

A denied call returns `host-error::denied(permission)`. Rate limited calls return `rate-limited`. Calls the app cannot serve return `unavailable`.

### Permissions

| Permission | Unlocks |
| --- | --- |
| `context-basic` | Current user, current channel, channel switch events |
| `context-timeline` | Message, reaction and typing events and `channel-messages` for server channels |
| `context-directory` | `list-guilds`, `list-channels`, `list-members` |
| `context-members` | `display-name` |
| `context-voice` | `voice-state` and `voice-changed` events |
| `context-private` | Direct message data in every context-timeline feature and in the render and filter hooks |
| `message-render` | `render-message` |
| `message-send` | `before-send`. The UI shows an indicator when a plugin altered a message. A rewrite cannot change the channel or reply target |
| `message-filter` | `filter-message` |
| `user-display` | `display-user` |
| `actions-send` | `send-message` |
| `actions-react` | `add-reaction` |
| `actions-navigate` | `open-channel` |
| `actions-clipboard` | `clipboard-write` |
| `actions-notify` | `notify` |
| `actions-open-url` | `open-url` |
| `storage` | The four `storage-*` functions |
| `net-fetch` | `fetch` to approved hosts |
| `ui-slots` | `register-slot` and `register-command` |

## Trees instead of recursive nodes

WIT has no recursive types, so markdown and UI nodes are flat. An `md-tree` or `ui-tree` is a `nodes` list plus `roots`, and every container refers to its children by index. A child must have a higher index than its parent, which keeps the structure acyclic. The host rejects trees that break the rule, are too deep (64 levels for markdown), too large (50,000 markdown nodes, 2,000 UI nodes) or point at out of range indices. The SDK hides this: `Md` and `Ui` are ordinary recursive Rust types converted on the way in and out.

## UI slots

`register-slot(slot, tree)` publishes a declarative tree under a slot name such as `message-action`. The UI reads the slot registry and renders the tree. When the user interacts with a node that has an id (button, input, toggle or a box), the host calls `on-ui-action(id, payload)`. A non-empty tree returned from it replaces the tree of the slot that owns that id. Images must use `https://` URLs and links `http://` or `https://`.

## Sandbox

- One wasmtime instance per plugin on its own thread, with epoch interruption ticking every 10 ms.
- Linear memory is capped at 64 MB and tables at 10,000 elements. Exceeding the memory cap traps the plugin.
- WASI preview 2 is linked with an empty context: no preopened directories, no environment, no arguments, no sockets, stdio goes nowhere. It exists so that `std` works for guests, not to give them capabilities.
- A trap, timeout or memory trap restarts the instance (new store, `init` runs again) after a backoff that starts at 100 ms and doubles up to 5 s. Five crashes within 60 seconds disable the plugin until it is enabled again. Plugin storage survives restarts.
- Each plugin has a queue. Events are dropped oldest first beyond 256 queued events. Interactive calls (render, send, filter, display, ui action, command) jump ahead of queued events.

## Writing a plugin

Create a library crate with `crate-type = ["cdylib"]`, depend on `oxidcord-plugin-sdk`, implement `Plugin` and call `export_plugin!`.

```rust
use oxidcord_plugin_sdk::{Md, MessageView, Plugin, export_plugin, host};

#[derive(Default)]
struct Example;

impl Plugin for Example {
	fn render_message(&mut self, msg: &MessageView, mut ast: Vec<Md>) -> Option<Vec<Md>> {
		let age_secs = (host::now_ms() - msg.timestamp_ms) / 1000;
		ast.push(Md::subtext(format!("sent {age_secs} s ago")));
		Some(ast)
	}
}

export_plugin!(Example);
```

Every method of `Plugin` has a no-op default. Helpers: `log::info!` and friends, `storage::get`, `storage::set`, `host::send_message`, and the `ui` builders (`ui::column().child(ui::text("x")).button("id", "label")`). See `plugins/hello-timestamps` for a complete plugin and `plugins/conformance-probe` for the misbehaving one the conformance runner uses.

## Conformance

`plugin_host::conformance::run(plugins_dir)` loads every plugin in a directory with its manifest permissions granted, then checks:

- ungranted calls to every gated host function return `denied`, and `send-message` is rate limited after a burst of three;
- `init` succeeds and each hook the manifest has a permission for answers within its deadline;
- `settings-schema` returns valid JSON;
- a `setting-changed` event with key `oxidcord.conformance.spin` that makes the plugin loop forever is interrupted within the `on-event` deadline and the instance restarts;
- a `setting-changed` event with key `oxidcord.conformance.alloc` that makes the plugin allocate past the memory cap traps and the instance restarts.

Well behaved plugins ignore the two probe keys and those checks are reported as skipped.
