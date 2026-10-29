# Message interception

Two hooks change messages: `before-send` sees what the user sends, `intercept-message` changes how received and cached messages are shown. Each has a plain form (1.0 and 1.1) and a full form (1.2) that a plugin selects by listing `before-send` or `intercept` in the manifest `hooks` array. Neither form can change what other people see beyond the text the user sends; both only affect this client.

## Outgoing: `before-send`

Needs `message-send`. The host calls it on a background thread before the REST request, for every message the user sends from the composer, including messages in direct messages (see "Direct messages" in `overview.md`). Plugins run in priority order and each sees the output of the previous one.

| Return | Effect |
| --- | --- |
| `pass` | No change |
| `replace(outgoing-message)` | Send this content instead. The channel and reply target must stay the same and the content must be 1 to 4000 bytes, otherwise the rewrite is ignored. The message shows an "edited by plugin" marker |
| `cancel(reason)` | The message is not sent and shows as failed with the plugin's reason (clipped to 200 bytes) |

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

The plain form is skipped for a message without text (a message that only carries files or stickers).

### `before-send-full`

A plugin that lists `before-send` in `hooks` gets `before-send-full(outgoing-full)` instead. It sees the whole message and may change all of it.

| Field | Meaning |
| --- | --- |
| `channel-id` | Target channel |
| `content` | Text |
| `reply-to` | Message being replied to |
| `mention-reply` | Whether the reply pings the author |
| `silent` | Sent as a silent message (no notification for others) |
| `attachments` | `outgoing-attachment`: `handle` (an opaque id; the plugin never gets file bytes or paths), `name`, `size`, `content-type`, `spoiler` |
| `stickers` | Sticker ids |

Rules for `replace(outgoing-full)`:

- `channel-id`, `reply-to` and the set of attachment handles must stay as they are, otherwise the rewrite is ignored.
- `content` is clipped to 4000 bytes. At least one of text, an attachment or a sticker must remain.
- `name` and `spoiler` of an attachment may change. The name is reduced to its last path component, control characters are removed and it is clipped to 255 bytes; `size` and `content-type` always keep the original values.
- `stickers` are deduplicated, capped at 3, and the session drops stickers that are not known or not available. Stickers go with the message when it has no files, and are sent as separate messages after it otherwise.
- `silent` and `mention-reply` may change.
- `cancel(reason)` behaves like the plain form.

Plain and full plugins form one chain in priority order. A plugin without `message-send` is not called. A plugin that sends with `send-as-user` does not see its own message in `before-send`, other plugins do. A message sent with `send-message` or `send-message-files` skips `before-send` of every plugin.

## Incoming: `intercept-message`

Needs `message-intercept`, and for direct messages also `context-private`. The host calls it on the render thread for every message that enters the cache, every edit, and every message rendered again after a plugin change or an `invalidate-message`. It receives a `message-full` and the current content as a `rich-tree`, and returns a `message-patch`.

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
| `visibility` | `shown`, `hidden(reason)` or `collapsed(reason)` | Hidden and collapsed messages always get a line naming the plugin and a Show control. Reason 200 bytes |
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

### `intercept-message-ext`

A plugin that lists `intercept` in `hooks` gets `intercept-message-ext(msg, meta, content)` instead of `intercept-message`, and returns a `message-patch-ext`. It is the same call plus a `message-meta` and extra fields next to `base: message-patch`.

`message-meta`: `guild-id`, `reply-to` (a `message-ref`), `flags`, `latency-ms` (send latency of live messages: measured for your own, id minus nonce for other people's), `author-color` (role color of the author), `pinned`, `edited-ms`, `nsfw-channel`.

`message-patch-ext` extras:

| Field | Effect | Limits |
| --- | --- | --- |
| `attachments` | `attachment-override` per attachment id: `blur`, `alt`, `tooltip`, `url` (same host only), `hidden` | 10 per message, unknown ids dropped. `alt` 1000 bytes, `tooltip` 200 bytes |
| `layout` | `default`, `stacked` or `inline-video` | |
| `reply-annotation` | Text at the end of the reply quote | 200 bytes |
| `reveal-spoilers` | Draws spoilers of the message uncovered | |
| `tooltip` | Hover hint on the message row | 200 bytes |
| `code-blocks` | `(index, code-block-node)`: colored lines for the code block with that ordinal. `code-block-node` is an optional `language` and `lines` of `code-span` (`text`, `color`, `bold`, `italic`) | 2000 lines, 128 KiB of text, 400 spans per line, 512 bytes per span, 32 bytes language. An index the message has no code block for is dropped |

How the shell draws each extra, and how overrides from several plugins merge, is described under "Message patches" in `views.md`.

### Batch form

For bulk renders (opening a channel, rendering after a plugin change) the render worker calls `intercept-messages-ext(items)` with up to 16 `(message-full, message-meta, rich-tree)` tuples and expects one `message-patch-ext` per item, in order. A single message goes through `intercept-message-ext`. The default SDK implementation of the batch calls `intercept-message-ext` for each item. A list of a different length is ignored.

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
3. `intercept-message` or `intercept-message-ext` of every extended plugin with `message-intercept`, chained by priority. Each sees the content produced so far. Patches are merged: content comes from the last plugin that set it, the first non-`keep` action wins per embed, the first `hidden` or `collapsed` visibility wins, decorations take the first plugin that set each field, and annotations, extra embeds, hidden attachments and accessories are shown from every plugin. The `message-patch-ext` extras merge as described in `views.md`.

Plugins are ordered by the manifest `priority`, lower first, then by id.

## Budgets

| Limit | Value |
| --- | --- |
| `intercept-message` and `intercept-message-ext` deadline per plugin | 50 ms |
| `intercept-messages-ext` deadline per plugin | 50 ms per message, at most 500 ms |
| Total budget per message | 150 ms, remaining plugins are skipped for that message |
| Total budget per batch | 150 ms per message, at most 750 ms |
| Circuit breaker | After 3 timeouts in a row a plugin's intercept hook is no longer called until the plugin is reloaded |
| `before-send` and `before-send-full` deadline per plugin | 500 ms |

A timeout or trap skips the plugin and the chain continues with the previous output. Rendering happens off the UI thread: the parsed message is shown first and replaced when the plugins finish. Loads and `fetch` are not available in these hooks (see "Call contexts" in `overview.md`), so a plugin that needs data from the network fetches it from a timer or an event and calls `invalidate-message` when it arrives.
