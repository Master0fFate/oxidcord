# Show Me Your Name

Changes how names are written next to messages, in the member list, in the profile popout and in the direct message list: the username and the nickname together, or the username alone.

Settings:
- `mode`: `user-nick` (default) writes the username then the nickname in a muted suffix, `nick-user` the nickname then the username, `user` only the username.
- `friendNicknames`: `dms` (default) uses the nickname you gave a friend only in direct messages, `always` prefers it over the server nickname everywhere, `fallback` uses it only where there is no server nickname.
- `displayNames`: use display names in place of usernames. Default off.

The nickname is the server nickname, or your friend nickname per the setting above, or the display name when there is none. When the username and the nickname are the same text only one is shown.

Limits: the Vencord setting `inReplies` is not part of this port, the client applies the name to messages, reply previews, the member list and the popout together. Streamer mode does not shorten the username. Only one display plugin can set the name of a user, if another enabled plugin returns an override for the same user first, this one is skipped for that user.

Permissions: `user-display`, `context-roles`, `context-relationships`, `storage`.

Port of the Vencord plugin showMeYourName.
