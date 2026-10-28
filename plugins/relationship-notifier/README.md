# Relationship Notifier

Sends a notification when a friend removes you, a friend request is withdrawn, you are removed from a server or a group chat is closed on you. The names come from a cache kept in the plugin storage, so the name is still known after the friend, server or group is gone from the client.

At startup (five seconds after the plugin loads, and again whenever the client reconnects) the cache of the last run is compared with what the client lists now, so removals that happened while the app was closed are reported too. Several removals in the same second are sent as one notification with up to five lines.

Settings:
- `notices`: also show a notice on screen, besides the notification. Default off.
- `offlineRemovals`: report removals found at startup. Default on.
- `friends`: friends removing you. Default on.
- `friendRequestCancels`: withdrawn friend requests. Default on.
- `servers`: leaving or being removed from a server. Default on.
- `groups`: group chats that were closed. Default on.

Limits: the client gives the plugin the event but not who caused it. Removing a friend yourself, leaving a server yourself or closing a group chat yourself is reported the same way as the other side doing it, Vencord skips those. Servers that become unavailable because of an outage are not reported. The notification title is fixed and notifications are limited to one every five seconds, which is why messages are grouped.

Permissions: `context-relationships`, `context-directory`, `context-private`, `actions-notify`, `ui-mount`, `storage`.

Port of the Vencord plugin relationshipNotifier.
