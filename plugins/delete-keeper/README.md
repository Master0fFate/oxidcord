# Delete Keeper

Keeps the last content it saw of the most recent 500 messages (least recently seen ids are dropped first). When one of them is deleted, the content moves to a list of the last 25 deletions and the `message-accessory` slot shows it under a `deleted` badge. Each entry is a box with the id `deleted-<message id>` so the host can place it under the right message. It never hides or alters messages.

Permissions: `context-timeline`, `storage`, `ui-slots`.
