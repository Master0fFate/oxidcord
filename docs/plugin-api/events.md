# Events

Plugins receive two kinds of events. The 1.0 `event` variant goes to `guest.on-event` and is available to every plugin. The 1.2 `client-event` variant goes to `hooks2.on-client-event` and is available to extended plugins that export `hooks2`. Both are fire and forget with a 200 ms deadline, are queued per plugin (events are dropped oldest first beyond 256 queued items), and never block the client.

A plugin only sees events its permissions allow. Events about direct messages and group DMs additionally need `context-private`; see "Direct messages" in `overview.md`.

## `event` (1.0)

| Variant | Payload | Permission | Produced when |
| --- | --- | --- | --- |
| `message-created` | `message-view` | `context-timeline` | A message arrives from the gateway |
| `message-updated` | `message-view` | `context-timeline` | A message is edited. Updates the client has no cached copy of are rebuilt from the update when they carry enough data |
| `message-deleted` | `message-ref` | `context-timeline` | A message is deleted. A bulk delete sends one event per message |
| `reaction-added` | `reaction-event` | `context-timeline` | Anyone adds a reaction. `emoji` is the unicode character, or `name:id` for a custom emoji |
| `typing` | `typing-event` | `context-timeline` | The gateway reports that a user started typing |
| `channel-switched` | `channel-switch` | `context-basic` | You open another channel |
| `voice-changed` | `voice-info` | `context-voice` | The state of the call you are in changes. Leaving the call sends no event, read `voice-state` to find out |
| `setting-changed` | `setting-change` | none | A setting of the plugin changes on its settings page. Always delivered, runs in a cache-only context |
| `connection` | `string` | none | `connected` or `disconnected` for the gateway connection. Always delivered |

Direct message events (message, reaction, typing and voice) are only delivered to plugins with `context-private`. Typing in a DM is decided by the channel, not by a server id.

## `client-event` (1.2)

| Variant | Payload | Permission | Produced when |
| --- | --- | --- | --- |
| `relationship-changed` | `relationship` | `context-relationships` | A friend, block or request is added or its type changes |
| `relationship-removed` | `snowflake` | `context-relationships` | A relationship is removed. The id is the other user |
| `guild-joined` | `snowflake` | `context-directory` | A server appears that the client did not know before |
| `guild-left` | `snowflake` | `context-directory` | The gateway removes a server you were in. Also fires when you are removed from it. Not sent when the server only becomes unavailable |
| `guild-removed` | `snowflake` | `context-directory` | Reserved. Never produced, see below |
| `dm-closed` | `snowflake` | `context-directory` and `context-private` | A DM or group DM channel is deleted |
| `presence-changed` | `presence` | `context-presence` | The presence of you or of a friend changes |
| `idle-changed` | `bool` | `context-presence` | The system idle time crosses 10 minutes in either direction. Checked every 30 s, only while a plugin listens, Windows and macOS only |
| `reaction-removed` | `reaction-event` | `context-timeline` | Anyone removes a reaction |
| `message-acked` | `tuple<message-ref, u32>` | `context-timeline` | The server confirms a message you sent from this client. The number is the delay in milliseconds between sending and the confirmation |
| `unread-changed` | `read-state` | `context-read-state`, plus `context-private` for DMs | The read state of a channel changes. Throttled, see below |
| `channel-list-changed` | none | `context-directory` | A channel is created, updated or deleted, a server joins or leaves, or the server layout changes. Coalesced |
| `ready` | none | none | The gateway finished its sync and the plugins have started |

Notes:

- `message-acked`, `reaction-removed` and `unread-changed` for DM channels, and `dm-closed`, need `context-private`. Messages and reactions in servers need only `context-timeline`.
- `guild-removed` exists so a future release can tell a kick from a server delete. Today every `GUILD_DELETE` for a server you were in is `guild-left`, and the removal of a server therefore arrives as `guild-left`. Plugins should treat the two the same.
- `ready` is sent when the gateway finishes its sync (READY) and the plugins have started. A plugin that is installed or enabled afterwards does not receive it; read the state with the functions of `data.md` in `init` instead.
- `presence-changed` is only produced for the current user and for friends. Other members' presences stay available through `user-presence` but produce no event.

### Throttling

| Event | Behaviour |
| --- | --- |
| `unread-changed` | Changes are collected per channel in a queue of 256 channels (a channel appears once). The queue is drained at one event per 100 ms tick, so a burst of unread updates reaches plugins as a steady trickle with the latest state of each channel |
| `channel-list-changed` | At most one event per 100 ms |
| `idle-changed` | Polled every 30 s, only sent when the idle state flips |

## Notification hook pipeline

`on-notification` lets a plugin change or drop the desktop notification and the chime for an incoming message. It needs `notifications-filter`, the manifest `hooks` entry `notification`, and for DM messages `context-private`.

1. A message that qualifies for a notification creates an alert in the client state. While at least one plugin declares the `notification` hook, the alert is created as pending and the shell does not show it or play its sound.
2. The session builds a `notification-info` and runs `on-notification` for each plugin that sees the message, in plugin order, on the interactive job lane:

| Field | Meaning |
| --- | --- |
| `id` | Alert id |
| `channel-id`, `guild-id` | Where the message arrived |
| `message` | The `message-ref` |
| `author` | Author id |
| `kind` | `message`, `dm` or `group-dm`. DM kinds are only delivered to plugins with `context-private` |
| `mentions-me` | The message mentions you directly or through a role |
| `mentions-everyone` | `@everyone` or `@here` |

3. Each plugin answers with a `notification-verdict`:

| Verdict | Effect |
| --- | --- |
| `allow` | No change, the next plugin is asked |
| `silent` | The notification shows without a sound |
| `suppress` | No notification and no sound |
| `replace(notification-override)` | A different title and body (clipped to 100 and 500 bytes, control characters removed, an empty title is ignored) and the `sound` flag it carries |

   The first verdict other than `allow` ends the chain. An error, a trap or a timeout counts as `allow`.
4. The session applies the verdict to the alert and the shell shows or drops it.

Timing: the whole chain has a budget of 250 ms, checked before each plugin starts, and each plugin call has the same 250 ms deadline. If the verdict has not arrived 500 ms after the alert was created, the session resolves the alert as `allow`. If no plugin declares the hook any more, pending alerts are released as `allow` at once. Three timeouts in a row disable the hook for that plugin until it is reloaded.
