# Message interception

Two hooks change messages: `before-send` sees what the user sends, `intercept-message` changes how received and cached messages are shown. Neither can change what other people see, both only affect this client.

## Outgoing: `before-send`

Needs `message-send`. The host calls it on a background thread before the REST request, for every message the user sends from the composer. Plugins run in priority order and each sees the output of the previous one.

| Return | Effect |
| --- | --- |
| `pass` | No change |
| `replace(outgoing-message)` | Send this content instead. The channel and reply target must stay the same and the content must be 1 to 4000 bytes, otherwise the rewrite is ignored. The message shows an "edited by plugin" marker |
| `cancel(reason)` | The message is not sent and shows as failed with the plugin's reason |

```rust
impl Plugin for Shout {
	fn before_send(&mut self, msg: OutgoingMessage) -> SendDecision {
		if let Some(rest) = msg.content.strip_prefix("!shout ") {
			return SendDecision::Replace(OutgoingMessage {
				content: rest.to_uppercase(),
				..msg
			});
		}
		SendDecision::Pass
	}
}
```

Messages a plugin sends with `send-message` or `send-as-user` skip `before-send`.

## Incoming: `intercept-message`

Needs `message-intercept`, and for direct messages also `context-private`. The host calls it on the render thread for every message that enters the cache, every edit, and every message rendered again after a plugin change. It receives a `message-full` and the current content as a `rich-tree`, and returns a `message-patch`.

### `message-full`

| Field | Meaning |
| --- | --- |
| `view` | The 1.0 `message-view`: ids, author, raw content, timestamp, flags |
| `attachments` | id, filename, content type, size, url, width, height |
| `embeds` | kind, title, description, url, color, provider, image and thumbnail urls, in message order |
| `stickers` | id, name, format |
| `reactions` | emoji, count, whether the user reacted |
| `mentions` | Mentioned user ids |
| `mine` | The current user wrote it |
| `lifecycle` | `live`, `edited(previous contents)` or `deleted(at-ms)` |

`edited` lists the earlier contents the client saw since it started, oldest first, at most ten.

### `message-patch`

| Field | Effect | Limits |
| --- | --- | --- |
| `content` | Replaces the rendered content. The next plugin sees this output | |
| `visibility` | `shown`, `hidden(reason)` or `collapsed(reason)` | Hidden and collapsed messages always get a line naming the plugin and a Show control |
| `annotations` | Lines under the message, each credited with the plugin name | 4 per plugin, 200 bytes each |
| `decoration` | Accent stripe color, background color, faded, strike-through | |
| `embeds` | One action per embed of the message, by index: `keep`, `hide` or `replace(embed-card)` | |
| `extra-embeds` | Embed cards added under the message | 3 per plugin |
| `hidden-attachments` | Attachment ids not to show | 10 |
| `accessory` | A view tree under the message, events arrive with point `message-accessory` | |
| `retain` | Keep this message visible after it was deleted | needs `message-retain`, only read for `deleted` messages |

In the SDK, `Patch::keep()` is the empty patch and builder methods add to it:

```rust
use oxidcord_plugin_sdk::model::MessageFull;
use oxidcord_plugin_sdk::patch::Patch;
use oxidcord_plugin_sdk::{Color, Md, PluginExt};

impl PluginExt for Highlighter {
	fn intercept_message(&mut self, msg: &MessageFull, content: Vec<Md>) -> Patch {
		if !msg.view.content.contains("deploy") {
			return Patch::keep();
		}
		Patch::keep()
			.content(vec![Md::colored(content, Color::token("warning"))])
			.accent(Color::token("warning"))
			.annotate("Mentions a deploy")
	}
}
```

### Rich content

`rich-tree` extends the markdown tree with two node kinds that only plugins produce:

| Node | SDK | Meaning |
| --- | --- | --- |
| `inline-image` | `Md::InlineImage { url, alt, width, height }`, `Md::inline_image(url, alt)` | An image inside the text, sized like an emoji when width and height are 0, at most 160 px. The image rules in `overview.md` apply |
| `styled` | `Md::Styled { children, color, background, mono }`, `Md::colored(children, color)` | Colors (theme tokens or RGBA) and a monospace font for a span |

Bad nodes (unknown color tokens, disallowed image hosts, broken indices) are dropped individually.

## Lifecycle and deleted messages

A deleted message normally disappears. While a running plugin holds both `message-retain` and `message-intercept`, the client keeps deleted messages in the cache and calls `intercept-message` with `lifecycle = deleted(at)`. The message stays in the chat, faded and marked as deleted, only when a plugin with `message-retain` returns `retain: true`. The 1.0 mechanism still works too: a plugin with `message-render` and `context-timeline` that returns a tree from `render-message` for a message with `DELETED_FLAG` keeps it.

```rust
fn intercept_message(&mut self, msg: &MessageFull, _content: Vec<Md>) -> Patch {
	match &msg.lifecycle {
		Lifecycle::Deleted(_) => Patch::keep().retain().annotate("Deleted"),
		Lifecycle::Edited(previous) if !previous.is_empty() => {
			Patch::keep().annotate(format!("{} earlier versions", previous.len()))
		}
		_ => Patch::keep(),
	}
}
```

## Ordering

For each message the render thread runs, in this order:

1. `filter-message` of every plugin with `message-filter`. Any `false` hides the message and nothing else runs.
2. `render-message` of every plugin with `message-render`, chained by priority.
3. `intercept-message` of every extended plugin with `message-intercept`, chained by priority. Each sees the content produced so far. Patches are merged: content comes from the last plugin that set it, the first non-`keep` action wins per embed, the first `hidden` or `collapsed` visibility wins, decorations take the first plugin that set each field, and annotations, extra embeds, hidden attachments and accessories are shown from every plugin.

Plugins are ordered by the manifest `priority`, lower first, then by id.

## Budgets

| Limit | Value |
| --- | --- |
| `intercept-message` deadline per plugin | 50 ms |
| Total budget per message | 150 ms, remaining plugins are skipped for that message |
| Circuit breaker | After 3 timeouts in a row a plugin's `intercept-message` is no longer called until the plugin is reloaded |
| `before-send` deadline per plugin | 500 ms |

A timeout or trap skips the plugin and the chain continues with the previous output. Rendering happens off the UI thread: the parsed message is shown first and replaced when the plugins finish.
