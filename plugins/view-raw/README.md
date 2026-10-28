# View Raw

Opens a window with the raw data of a message, a member or a channel as JSON, with buttons that copy the JSON (and for a message its plain content).

- Messages: a View raw button in the message toolbar (`clickMethod` button) or an entry in the message menu (`clickMethod` menu). The data comes from the client's own view of the message: author, content, timestamps, flags, attachments, embeds, stickers, reactions, mentions, lifecycle and the extra message data (reply target, latency, pinned, author color).
- Members: View raw in the member context menu, in servers only. Shows the member data of the client (nick, roles, joined date, timeout, boost).
- Channels: View raw in the channel context menu.

The JSON is built inside the plugin from what the client exposes, it is not the Discord payload, so field names are the client's own.

Settings:
- `clickMethod`: `button` (default) or `menu`, where the message entry lives.
- `messageContextMenu`: also add the message menu entry when `clickMethod` is button. Default off.
- `sortKeys`: sort the keys of every object alphabetically. Default on.

Limits: `stripSensitiveData` is dropped, nothing sensitive is exposed to plugins. Servers, roles and profiles have no View raw. Direct messages are not available. Lines longer than about 3.5 KB are cut in the window, Copy has everything. At most about 140 KB are drawn.

Permissions: `ui-mount`, `context-timeline`, `context-roles`, `context-directory`, `actions-clipboard`, `storage`.

Port of the Vencord plugin viewRaw.
