# First-party plugins

Every plugin here is a workspace member, builds with `cargo build -p <name> --target wasm32-wasip2 --release` and passes the conformance suite. The API is described in `docs/plugin-api/`. Each manifest declares the lowest API version the plugin needs (`^1.0`, `^1.1` or `^1.2`). Plugins that use views, menus, patches or the 1.2 hooks enable the SDK feature `ext`.

106 plugins are listed, including the test fixture `conformance-probe`. 91 of them were written as ports of Vencord plugins against the Oxidcord API, and `fake-nitro`, `message-logger`, `see-hidden-channels` and `silent-type` are equivalents of four more. The limitations of the ports are listed in `docs/plugin-api/porting-vencord.md`, and each plugin folder has its own README with the settings and limits.

Since API 1.1, `send-message` only works while the plugin handles a user action (a button, input, menu item, slash command or keybind) and at most three times per action. `quick-replies` sends from a click or `/snip` and is not affected.

## Plugins

### Messages

How received messages look and what you can do with them.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `hello-timestamps` | Relative send time under each message and a Copy ID button. | `message-render`, `ui-slots`, `actions-clipboard` |
| `spoiler-all` | Hides every message behind a spoiler, switched from the composer toolbar. | `message-render`, `ui-slots`, `storage` |
| `keyword-highlighter` | Bolds your keywords in messages, adds a keyword badge and notifies on a match. | `message-render`, `storage`, `context-basic`, `actions-notify` |
| `message-logger` | Keeps deleted messages visible and the earlier versions of edited ones one click away. | `context-timeline`, `message-render`, `storage`, `ui-slots` |
| `word-mute` | Hides messages that contain words or phrases you muted. | `message-filter`, `storage`, `ui-slots`, `context-basic` |
| `no-blocked-messages` | Hides messages from blocked users and from a list of user ids you keep. | `message-filter`, `context-relationships`, `context-private`, `storage` |
| `mention-avatars` | Shows user avatars inside mentions. | `message-intercept`, `context-members`, `storage` |
| `message-latency` | Shows how long your own messages took to send when it was slow. | `message-intercept`, `storage` |
| `message-link-embeds` | Previews a message that a message links to as a card with a Jump button. | `message-intercept`, `context-timeline`, `context-directory`, `actions-navigate`, `actions-compose`, `storage` |
| `message-click-actions` | Double click to edit or reply, Ctrl+Alt click to delete your own message. | `ui-input`, `context-timeline`, `actions-compose`, `actions-delete`, `storage` |
| `dearrow` | Replaces sensationalist YouTube embed titles and thumbnails with the DeArrow ones. | `message-intercept`, `net-fetch`, `storage` |
| `valid-user` | Loads the names of mentioned users the client does not know. | `message-intercept`, `context-members` |
| `who-reacted` | Shows who reacted to a message after a click on a toolbar button or the reaction menu. | `ui-mount`, `context-timeline`, `message-intercept`, `storage` |
| `reply-timestamp` | Shows a timestamp on reply previews. | `message-intercept`, `storage` |
| `reveal-all-spoilers` | Ctrl-click reveals all spoilers of a message, Ctrl+Shift-click all of the channel. | `ui-input`, `message-intercept`, `context-timeline` |
| `code-highlight` | Syntax highlighting for code blocks in 16 languages. | `message-intercept`, `storage` |
| `unindent` | Trims leading indentation from code blocks you send and receive. | `message-send`, `message-render` |
| `translate` | Translates messages with Google Translate or DeepL. | `ui-mount`, `net-fetch`, `message-intercept`, `context-timeline`, `composer`, `storage` |
| `more-quick-reactions` | Adds a row of your most used reactions to the message hover bar. | `ui-mount`, `actions-react`, `storage`, `context-timeline`, `context-basic` |
| `super-reaction-tweaks` | Adds Super react to the reaction menu and can make super reactions the default. | `ui-mount`, `actions-react`, `actions-settings`, `storage` |

### Composer

What you type and send.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `link-cleaner` | Strips tracking parameters from links before a message is sent. | `message-send` |
| `clear-urls` | Removes tracking parameters from links you send, using the ClearURLs rules. | `message-send`, `net-fetch`, `storage` |
| `text-shortcuts` | Expands emoticons like :shrug: and your own ;shortcuts before sending. | `message-send`, `storage`, `ui-slots` |
| `text-replace` | Replaces text in your messages with string and regex rules. | `message-send`, `storage`, `ui-mount` |
| `send-timestamps` | Timestamp picker button and backtick shortcut for Discord timestamps. | `ui-mount`, `composer`, `message-send`, `storage` |
| `fake-nitro` | Sends emoji you cannot use as image links and shows received emoji links inline. | `composer`, `context-expressions`, `message-send`, `message-intercept` |
| `quick-replies` | Saved snippets you send from a composer popover or with /snip. | `ui-slots`, `storage`, `actions-send`, `context-basic` |
| `custom-commands` | Your own slash commands that send saved text. | `ui-slots`, `ui-mount`, `storage`, `actions-send` |
| `quick-mention` | Message toolbar button that puts a mention in the composer. | `ui-mount`, `composer` |
| `quick-reply` | Ctrl+Up and Ctrl+Down reply to messages, with Shift they edit your own. | `ui-keybinds`, `context-basic`, `context-timeline`, `context-relationships`, `actions-compose`, `storage` |
| `no-reply-mention` | Turns the reply ping off by default, with lists of exceptions. | `message-send`, `context-timeline`, `context-roles`, `storage` |
| `silent-message-toggle` | Composer button to send the next message silently. | `message-send`, `ui-mount`, `context-basic`, `storage` |
| `silent-type` | Hides that you are typing from other people. | `typing-hidden` |
| `anonymise-file-names` | Renames uploaded files to random, consistent or timestamp names. | `message-send`, `ui-mount`, `storage` |
| `preview-message` | Shows a rendered preview of your draft above the composer. | `ui-mount`, `composer` |
| `character-counter` | Character counter for the message box. | `composer-watch`, `ui-mount`, `storage` |
| `fav-emoji-first` | Puts the emoji you sent most recently first in the emoji autocomplete. | `ui-input`, `context-expressions`, `message-send`, `storage` |
| `no-server-emojis` | Keeps server emoji out of emoji autocomplete. | `ui-input`, `context-basic`, `storage` |
| `sticker-paste` | Composer button that attaches a picked sticker to your draft instead of sending it at once. | `ui-mount`, `context-expressions`, `composer` |
| `petpet` | Adds /petpet to send a petting gif made from a user avatar. | `ui-slots`, `context-attachments`, `actions-upload` |

### Users and profiles

Names, badges, profile popouts and the friends list.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `user-notes` | Private notes on people, shown as a badge next to their name. | `ui-slots`, `storage`, `user-display` |
| `user-pronouns` | Shows profile pronouns next to names. | `user-display`, `context-profiles`, `context-basic`, `storage` |
| `usrbg` | Shows USRBG banners in profiles. | `user-display`, `net-fetch`, `context-profiles`, `storage` |
| `fake-profile-themes` | Shows the profile colors people hide in their bio with 3y3 encoding. | `ui-mount`, `context-profiles`, `storage` |
| `decor` | Shows the avatar decorations people picked in Decor. | `user-display`, `net-fetch`, `storage` |
| `better-role-dot` | Colored badge next to names in the top role color, a click copies the color. | `user-display`, `context-roles`, `actions-clipboard`, `ui-mount`, `storage` |
| `better-role-context` | Role menu entries to copy the color and id and to view the role icon. | `ui-mount`, `context-roles`, `actions-clipboard`, `storage` |
| `irc-colors` | Gives every user a stable name color, like IRC clients. | `user-display`, `storage`, `context-roles` |
| `show-me-your-name` | Shows usernames next to nicknames, or usernames only. | `user-display`, `context-roles`, `context-relationships`, `storage` |
| `platform-indicators` | Platform icon next to online users, colored by status. | `user-display`, `context-presence`, `storage` |
| `user-voice-show` | Speaker badge next to users in the voice call you are in. | `user-display`, `context-voice`, `storage` |
| `show-timeout-duration` | Clock badge with the time left on a timeout next to timed out members. | `user-display`, `context-roles`, `storage` |
| `role-color-everywhere` | Colors names and mentions with the top role color, and can tint the message stripe. | `user-display`, `message-intercept`, `context-roles`, `storage` |
| `force-owner-crown` | Crown badge next to the name of the server owner, even in large servers. | `user-display`, `context-directory` |
| `show-connections` | Connected accounts in the profile popout. | `ui-mount`, `context-profiles`, `actions-clipboard`, `storage` |
| `review-db` | ReviewDB reviews about a user in the profile popout, read only. | `ui-mount`, `net-fetch`, `storage` |
| `mutual-group-dms` | Group DMs you share with a user, in the profile popout. | `ui-mount`, `context-directory`, `context-private`, `actions-navigate` |
| `view-icons` | View avatar, banner and icon entries in member and server menus. | `ui-mount`, `context-profiles`, `context-directory`, `storage` |
| `copy-user-urls` | Copy User URL entry in the user menu. | `ui-mount`, `actions-clipboard` |
| `account-server-profile` | A row above the account panel and a menu entry open your avatar, nickname, roles and join date in the open server. | `ui-mount`, `context-basic`, `context-roles`, `storage` |
| `implicit-relationships` | Implicit tab on the Friends page with people you share two or more servers with and no relationship. | `ui-mount`, `context-relationships` |
| `sort-friend-requests` | Requests by date tab on the Friends page with incoming and outgoing requests, newest first. | `ui-mount`, `context-relationships`, `storage` |

### Channels and servers

Channel list, server list, voice and server details.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `see-hidden-channels` | Shows channels you cannot open in the channel list, dimmed with a lock. | `channels-hidden` |
| `keep-current-channel` | Reopens the channel you were in before a restart. | `context-basic`, `storage`, `actions-navigate` |
| `typing-indicator` | Badge on channels and DMs while someone is typing there. | `ui-input`, `context-timeline`, `context-private`, `context-read-state`, `context-relationships`, `context-basic`, `context-members`, `storage` |
| `member-count` | Row above the member list with the online, total and voice counts, and the online and total counts in the server tooltip. | `ui-mount`, `context-directory`, `context-voice`, `storage` |
| `server-info` | Server details window from the server menu. | `ui-mount`, `context-directory` |
| `server-list-indicators` | Online friend count or server count in the server list. | `ui-mount`, `context-relationships`, `context-presence`, `context-directory`, `storage` |
| `permissions-viewer` | Shows the permissions of a user or channel and the roles of a server. | `ui-mount`, `context-roles`, `context-directory`, `context-basic`, `storage` |
| `new-guild-settings` | Applies your notification settings to servers when you join them. | `context-directory`, `actions-settings`, `storage` |
| `vc-double-click` | Join voice channels with a double click instead of a single click. | `ui-input`, `context-directory`, `context-voice`, `actions-voice`, `actions-navigate` |
| `call-timer` | Shows how long the current call has lasted in the voice panel. | `context-voice`, `ui-mount`, `storage` |
| `vc-narrator` | Speaks when people join, leave or move in your voice channel. | `context-voice`, `context-members`, `context-basic`, `actions-speak`, `storage` |
| `jump-to-top` | Button to jump to the first message of a channel. | `ui-mount`, `actions-compose` |
| `expression-cloner` | Clones emoji and stickers into a server you manage. | `ui-mount`, `context-directory`, `context-roles`, `context-expressions`, `actions-guild` |

### Attachments

Files, images, emoji and stickers.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `blur-nsfw` | Blurs attachments in age restricted channels until you hover them. | `message-intercept` |
| `fix-images-quality` | Loads attached images at their original resolution. | `message-intercept`, `storage` |
| `image-filename` | Shows the file name of image attachments as a tooltip. | `message-intercept`, `storage` |
| `no-mosaic` | Stacks image and video attachments instead of the mosaic grid. | `message-intercept`, `storage` |
| `hide-attachments` | Per-message button that hides attachments and embeds. | `ui-mount`, `message-intercept`, `storage` |
| `copy-file-contents` | Copy button on text file attachments. | `ui-mount`, `context-timeline`, `context-attachments`, `actions-clipboard` |
| `direct-file-download` | Save as button on attachments that opens the save dialog. | `ui-mount`, `actions-save` |
| `voice-download` | Download button on voice messages and other audio attachments. | `ui-mount`, `context-timeline`, `actions-save` |
| `copy-emoji-markdown` | Copies an emoji as formatted text from its menu. | `ui-mount`, `actions-clipboard`, `storage` |
| `copy-sticker-links` | Copies or opens the link of a sticker from its menu. | `ui-mount`, `context-expressions`, `actions-clipboard`, `actions-open-url` |
| `reverse-image-search` | Image search entries in the image menu. | `ui-mount`, `actions-open-url` |

### Presence and notifications

Status, notification behaviour and sessions.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `custom-idle` | Sets your status to idle after a time you choose. | `context-presence`, `actions-presence`, `context-basic`, `ui-mount`, `storage` |
| `one-ping-per-dm` | One notification sound per DM until you read it. | `notifications-filter`, `context-read-state`, `context-private`, `storage` |
| `read-all-notifications` | Button to mark every server as read. | `ui-mount`, `context-directory`, `context-read-state`, `actions-ack` |
| `relationship-notifier` | Notifies you when a friend, group chat or server removes you. | `context-relationships`, `context-directory`, `context-private`, `actions-notify`, `ui-mount`, `storage` |
| `better-sessions` | Lists your logged in devices on the plugin page with custom names and a notification for new sessions. | `context-sessions`, `ui-mount`, `actions-notify`, `storage` |
| `notification-volume` | Separate volume for the notification chime. | `actions-settings`, `storage` |

### Shell and client options

Changes to the client layout and settings.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `better-folders` | Shows the servers of an open folder in a sidebar next to the server list. | `actions-settings`, `storage` |
| `plain-folder-icon` | Plain folder icons instead of small server icons. | `actions-settings` |
| `pin-dms` | Pins DMs into categories at the top of the DM list. | `ui-mount`, `context-directory`, `context-private`, `context-timeline`, `actions-settings`, `storage` |
| `override-forum-defaults` | Default layout and sort order for forum channels. | `actions-settings`, `storage` |
| `show-all-message-buttons` | Always shows every button in the message toolbar. | `actions-settings` |
| `no-typing-animation` | Stops the animation of the typing dots. | `actions-settings` |
| `startup-timings` | Lists the client startup phases on the plugin settings page. | `context-basic`, `ui-mount` |
| `local-status-badge` | Status bar item with the channel id and a badge that cycles through three colors. | `context-basic`, `ui-slots` |

### Tools

Helpers and test fixtures.

| Plugin | What it does | Permissions |
| --- | --- | --- |
| `toolbox` | Title bar button with the plugin settings, the client version and the list of enabled plugins. | `ui-mount`, `plugins-list`, `context-basic`, `actions-navigate`, `actions-clipboard`, `storage` |
| `notes-panel` | Notes page in the server rail, a Save to notes message action and a status bar counter. | `ui-mount`, `storage`, `context-timeline` |
| `view-raw` | Shows the client data of a message, member or channel as JSON with copy buttons. | `ui-mount`, `context-timeline`, `context-roles`, `context-directory`, `actions-clipboard`, `storage` |
| `replace-google-search` | Search selected text with the engine of your choice. | `ui-mount`, `actions-open-url`, `storage` |
| `open-in-app` | Opens Spotify, Steam, Tidal and Apple Music links in their apps. | `actions-open-url`, `storage` |
| `conformance-probe` | Test fixture the conformance runner pushes to its limits, not for users. | none |

## Overlaps

Two pairs of plugins do similar things. They can run together, and each is the better choice in one case.

| Plugins | Difference |
| --- | --- |
| `clear-urls` and `link-cleaner` | `link-cleaner` is a small plugin with a fixed list of tracking parameters and no network access. `clear-urls` uses the full ClearURLs rule set, which it downloads and refreshes, so it knows many more sites but needs `net-fetch` |
| `custom-commands` and `quick-replies` | `quick-replies` keeps snippets you pick from a composer popover or send with `/snip <name>`. `custom-commands` turns saved text into slash commands of their own (`/tags create`, then `/<name>`) with arguments |

## Commands

Run them from the repository root.

```
cargo xtask plugin new <id> [--dir plugins]
cargo xtask plugin build <path> [--release]
cargo xtask plugin pack <path> [--out dist/plugins] [--sign <secret key file>]
cargo xtask plugin conformance [--dir plugins]
cargo xtask plugin catalog [--dir plugins] [--out plugins/catalog.json] [--packed dist/plugins] [--base-url <url>]
cargo xtask plugin keygen <out>
cargo xtask plugin sign-catalog <catalog.json> <secret key file>
```

- `new` scaffolds `Cargo.toml`, `plugin.toml`, `src/lib.rs` and `README.md` in `<dir>/<last part of the id>`.
- `build` builds the crate for `wasm32-wasip2` and prints the path of the `.wasm`. A directory without a `Cargo.toml` has nothing to build.
- `pack` builds in release mode and writes `<id>-<version>.oxidcord-plugin`. With `--sign` the archive carries a publisher signature.
- `conformance` packs every plugin in the directory into a temporary directory, installs the archives there and runs `plugin_host::conformance::run` over the result. It prints one row per plugin and exits non-zero when a check fails.
- `catalog` packs every plugin (except `conformance-probe`) into `--packed` and writes the catalog with the sha256 and length of each archive. `--base-url` is put in front of the archive name to form each `url`, without it the url is the file name. Entries start as `reviewed: false`.
- `keygen` writes a base64 ed25519 secret key (mode 0600 on unix, refuses to overwrite) and prints the base64 public key.
- `sign-catalog` writes `<catalog.json>.sig` next to the catalog: a base64 ed25519 signature over the exact bytes of the file. Hosts check it with `plugin_host::catalog::verify_catalog`, so change the catalog and the signature is void.
