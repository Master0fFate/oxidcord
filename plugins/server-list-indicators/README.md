# Server List Indicators

Shows a small count above the server list: how many of your friends are online, how many servers you are in, or both.

Settings:
- `mode`: `friends` (default), `servers` or `both`.

The counts are refreshed when a friend's presence, a relationship or your server list changes (with a two second delay so a burst counts once) and every 30 seconds. A friend counts as online when their status is anything but offline or invisible.

Limits: the server count is the number of servers the client lists. Vencord also adds servers you only asked to join, Oxidcord has no such list.

Permissions: `ui-mount`, `context-relationships`, `context-presence`, `context-directory`, `storage`.

Port of the Vencord plugin serverListIndicators.
