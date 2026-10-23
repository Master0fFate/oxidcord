# User Notes

`Add note` in the `user-context-menu` slot opens a `modal` slot with an input. The host passes the user id as the action payload. Save keeps the note per user id in plugin storage, an empty note deletes it. The first 20 characters of a note show as a badge next to the user's name.

Permissions: `ui-slots`, `storage`, `user-display`.
