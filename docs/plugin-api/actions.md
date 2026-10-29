# Actions

Extended plugins change the client through the `client` interface (API 1.2). In the SDK these are the functions of `oxidcord_plugin_sdk::actions`. Every write is gated by a permission and returns `result<_, host-error>`.

A plugin never talks to Discord directly. Each write becomes a typed command that the session executes with its own permission math and rate limiting, so a plugin cannot do more than the signed-in user could do in the interface, and never sees the token.

## Rules that apply to every write

- **User action.** Functions marked "user action" work only while the plugin handles real user input (a button, a menu item, a slash command, a keybind, a badge, message, channel or link click). Elsewhere they return `host-error::invalid("requires a user action")`. See "User actions" in `overview.md`.
- **Direct messages.** An action on a DM or group DM channel needs `context-private`. Without it the call fails. A channel the client does not know counts as a guild channel and is refused.
- **Rate limits.** Token buckets per plugin. A call over the limit returns `rate-limited`. "Shares X" in the table means the function draws from the same bucket as X.
- **Checks by the session.** The session checks the same things the interface would: own message for editing, own message or Manage Messages for deleting, Manage Expressions for cloning, a known and usable sticker for attaching.

## Functions

| Function | Permission | User action | Limit | What it does |
| --- | --- | --- | --- | --- |
| `set-status(status)` | `actions-presence` | no | burst 2, 1 per 5 s | Sets the account status. `status` is `online`, `idle`, `dnd` or `invisible`, anything else is `invalid("unknown status")` |
| `set-custom-status(text, emoji, expires-ms)` | `actions-presence` | no | shares `set-status` | Sets the custom status text (up to 128 characters) and optional emoji. `expires-ms` must be in the future. No other activity, game or rich presence can be set |
| `mark-read(channel-id)` | `actions-ack` | no | burst 5, 1 per s | Marks a channel as read |
| `mark-unread(message)` | `actions-ack` | no | shares `mark-read` | Moves the read marker so the message and everything after it is unread |
| `mark-guild-read(guild-id)` | `actions-ack` | yes | shares `mark-read` | Marks every channel of a server as read |
| `start-reply(message, mention)` | `actions-compose` | yes | burst 5, 1 per s | Puts the composer of that channel in reply mode, with the ping switched on or off |
| `start-edit(message)` | `actions-compose` | yes | shares `start-reply` | Opens the inline editor for one of your own messages |
| `jump-to-message(message)` | `actions-compose` | yes | shares `start-reply` | Opens the channel and scrolls the timeline to the message |
| `jump-to-top(channel-id)` | `actions-compose` | yes | shares `start-reply` | Opens the channel and scrolls to its first message, loading the oldest page when needed |
| `edit-message(message, content)` | `actions-edit` | yes | shares `start-reply` | Edits one of your own messages. 1 to 4000 bytes |
| `delete-message(message)` | `actions-delete` | yes | shares `start-reply` | Deletes your own message, or any message when you have Manage Messages |
| `react(message, emoji, burst)` | `actions-react` | only when `burst` | burst 5, 1 per s, shared with `add-reaction` | Adds a reaction. `burst` sends a super reaction |
| `remove-reaction(message, emoji)` | `actions-react` | no | shares `react` | Removes your reaction |
| `pick-files(multiple)` | `actions-upload` | yes | shares `start-reply` | Shows the native file dialog. Returns a handle, name, size and content type per file (one file, or up to 16 when `multiple`). The plugin never sees the path |
| `composer-attach-files(handles)` | `actions-upload` | yes | shares `start-reply` | Adds files from earlier `pick-files` calls (up to 10 handles) to the draft of the selected channel |
| `send-message-files(msg, files)` | `actions-upload` | yes | shares the `send-message` limits | Sends a message with files you give as bytes. Up to 4 files and 10 MiB in total. File names are validated: no path separators, control characters or `: * ? " < > |`, up to 255 bytes |
| `composer-set-draft(text)` | `composer` | yes | burst 5, 1 per s | Replaces the draft of the selected channel. Up to 4000 bytes |
| `composer-attach-sticker(sticker-id)` | `composer` | yes | shares `composer-set-draft` | Attaches a known, usable sticker to the draft. The sticker goes out with the message (up to 3) |
| `save-attachment(message, attachment-id)` | `actions-save` | yes | shares `start-reply` | Shows the native save dialog and writes the attachment. Up to 100 MiB |
| `join-voice(guild-id, channel-id)` | `actions-voice` | yes | shares `start-reply` | Joins a voice channel |
| `leave-voice()` | `actions-voice` | yes | shares `start-reply` | Leaves the current call |
| `set-guild-notifications(guild-id, settings)` | `actions-settings` | no | burst 5, 1 per 2 s | Changes the notification settings of a server |
| `set-channel-notifications(channel-id, settings)` | `actions-settings` | no | shares `set-guild-notifications` | Changes the notification override of a channel |
| `set-client-option(value)` | `actions-settings` | no | shares `set-guild-notifications` | Sets one client option, see below |
| `set-dm-sections(sections)` | `actions-settings` | no | shares `set-guild-notifications` | Groups DMs into sections, see below |
| `clone-emoji(guild-id, emoji, name)` | `actions-guild` | yes | shares `start-reply` | Creates a copy of a custom emoji in a server where you have Create Expressions or Manage Expressions. `name` is 2 to 32 letters, digits or underscores |
| `clone-sticker(guild-id, sticker-id)` | `actions-guild` | yes | shares `start-reply` | Creates a copy of a sticker in such a server |
| `register-keybind(bind)`, `unregister-keybind(id)` | `ui-keybinds` | no | `register` shares the 20 per second `ui` limit | Binds or unbinds a chord, see below |
| `speak(text, rate, volume)` | `actions-speak` | no | burst 1, 1 per 2 s | Reads text aloud. Up to 300 characters; `rate` is clamped to 0.5 to 2.0 and `volume` to 0.0 to 1.0. Windows only |
| `invalidate-message(message)`, `invalidate-user(user-id)`, `invalidate-channel(channel-id)` | none | no | burst 50, 1 per 100 ms, shared | Asks the client to run the plugin's hooks again, see below |
| `open-settings(page)` | `actions-navigate` | yes | shares `start-reply` | Opens a settings page. Pages: `my-account`, `profiles`, `privacy`, `devices`, `connections`, `clips`, `friend-requests`, `appearance`, `accessibility`, `voice-video`, `chat`, `notifications`, `keybinds`, `language`, `streamer-mode`, `advanced`, `plugins`, `themes`, `developer`, `updates` |
| `navigate-back()`, `navigate-forward()` | `actions-navigate` | yes | shares `start-reply` | Moves through the navigation history of the shell |

Every `message` argument is a `message-ref` (message id, channel id, optional server id). The 1.0 and 1.1 writes (`send-message`, `add-reaction`, `open-channel`, `clipboard-write`, `notify`, `open-url`, `composer-insert`, `send-as-user`) are listed in `overview.md`.

## `notification-settings`

All fields are optional; only the fields that are set change. A call that sets nothing does nothing.

| Field | Server | Channel | Meaning |
| --- | --- | --- | --- |
| `muted` | yes | yes | Mute or unmute |
| `mute-until-ms` | yes | yes | Mute until a time. Implies `muted` |
| `level` | 0 to 2 | 0 to 3 | 0 all messages, 1 only mentions, 2 nothing, 3 (channel only) use the server setting |
| `suppress-everyone` | yes | no | Suppress `@everyone` and `@here` |
| `suppress-roles` | yes | no | Suppress role mentions |
| `mobile-push` | yes | no | Mobile push notifications |
| `hide-muted` | yes | no | Hide muted channels |

A level above 3 is rejected with `invalid("notification level")`.

## Client options

`set-client-option` takes one variant of `client-option`. The values are stored under `plugins.client-options.<plugin>` and applied while the plugin is enabled. Disabling or uninstalling the plugin reverts them. Options of all enabled plugins are merged field by field; when two plugins set the same option, the one later in plugin order wins.

| Variant | Value | Effect |
| --- | --- | --- |
| `message-toolbar-expanded` | bool | The message hover toolbar always shows all buttons |
| `plain-folder-icons` | bool | Server folders show a plain folder icon instead of small server icons |
| `folder-sidebar` | bool | Open folders list their servers in a second column beside the rail |
| `forum-layout` | `u8`, 0 to 2 | Layout a forum channel opens with: 1 list, 2 gallery |
| `forum-sort` | `u8`, 0 to 1 | Sort order a forum channel opens with: 0 latest activity, 1 creation date |
| `notification-volume` | `u8`, 0 to 100 | Volume of the notification chime |
| `streamer-mode` | bool | Streamer mode, which hides invite cards |
| `typing-animation` | bool | Whether the typing dots animate |
| `super-react-default` | bool | The reaction picker sends super reactions |
| `reply-ping-default` | bool | Whether a new reply pings the author |

A value out of range returns `invalid("option value out of range")`.

## DM sections

`set-dm-sections` takes a list of `dm-section` (`title`, `channels`, `collapsed`). The DM list shows each section above the other DMs, with a collapsible header; `collapsed` is the state the section starts in. At most 16 sections, 200 channels in total and 64 bytes per title; sections with an empty title are dropped. An empty list removes the plugin's sections. Channels that are not DMs of the account are dropped, and a channel is listed once per section.

## Keybinds

`register-keybind` takes a `keybind` with a plugin-scoped `id` and a `chord`. The shell calls `on-keybind(id)` when the chord is pressed while the app window has focus. Keybinds are never global OS hotkeys, and a plugin keybind is checked before the shell's own shortcuts.

- A chord is modifiers and one key joined by `+`, for example `ctrl+alt+up`. Modifiers are `ctrl`, `alt`, `shift` and `meta`, each once, in any order and any case. The chord is stored in the order `ctrl+alt+shift+meta`.
- Keys are `a` to `z`, `0` to `9`, `f1` to `f12`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup` (or `pgup`), `pagedown` (or `pgdn`) and `,` (or `comma`).
- A chord needs `ctrl`, `alt` or `meta`, or must be an F-key. `shift` alone is not enough.
- Chords the client uses itself are refused with `invalid("chord is reserved by the app")`:

| Chord | Used for |
| --- | --- |
| `ctrl+k`, `meta+k` | Quick switcher |
| `ctrl+0`, `meta+0` | Zoom reset |
| `alt+up`, `alt+down` | Previous and next channel |
| `alt+f4`, `meta+q`, `ctrl+w`, `meta+w` | Closing the window and quitting |
| `ctrl+a`, `ctrl+c`, `ctrl+v`, `ctrl+x`, `ctrl+z`, `ctrl+y` and the `meta` versions | Text editing |
| `ctrl+,`, `meta+,` | Settings |

- A chord another plugin already holds returns `invalid("chord is bound by another plugin")`. Registering the same `id` again moves it to the new chord.
- An invalid chord returns `invalid("invalid keybind")`, and the 33rd keybind of a plugin returns `invalid("too many keybinds")`. Limit: 32 keybinds per plugin.
- `unregister-keybind` removes an id, and all keybinds of a plugin are removed when it is disabled.

## Invalidation

`invalidate-message`, `invalidate-user` and `invalidate-channel` tell the client that something the plugin's hooks depend on changed. They need no permission because they only cause hooks the plugin already holds to run again.

| Function | Effect |
| --- | --- |
| `invalidate-message` | The message is rendered again, so `intercept-message*` runs for it |
| `invalidate-user` | The display override of the user is computed again, so `display-user*` runs for it |
| `invalidate-channel` | The channel decoration is computed again, so `decorate-channel` runs for it |

The three functions share one bucket of 50 calls with a refill of 10 per second.

## Opening URLs and app schemes

`open-url` always accepts `http` and `https`, one call per 10 seconds, up to 2048 bytes, and the client opens them in the browser.

A plugin can also open links of other apps. The manifest lists the schemes it needs in `url_schemes`, picked from this fixed set, and the consent dialog shows each one:

| Scheme | Opens |
| --- | --- |
| `spotify` | Spotify |
| `steam` | Steam |
| `tidal` | Tidal |
| `itms-apps` | Apple App Store and iTunes Store links |
| `music` | Apple Music |

A URL with a scheme outside the manifest list, or one the user did not approve, returns `invalid("url scheme is not approved")`. Opening an app scheme is a user action. The same rules apply to the `open(url)` verdict of `on-link-click`. The manifest key needs the `actions-open-url` permission; see `building.md`.
