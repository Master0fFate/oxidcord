# No Blocked Messages

Hides every message written by a user you blocked, so not even the "blocked message" line is left in the chat. Messages from the users in the `ignoredUsers` list are hidden the same way.

Settings:
- `applyToIgnoredUsers`: also hide the users in `ignoredUsers`. Default on.
- `ignoredUsers`: user ids to treat like blocked users. Oxidcord has no ignore state of its own, so Discord's ignored users cannot be read and this list takes that place. Default empty.

The blocked list is read at startup and kept up to date from relationship events. Changing a setting renders the cached messages again.

Limits: the Vencord setting `ignoreMessages`, which also drops the messages before they count as unread, is not part of this port. A filter plugin only hides messages in the chat, unread badges and notifications still count them.

Permissions: `message-filter`, `context-relationships`, `context-private`, `storage`.

Port of the Vencord plugin noBlockedMessages.
