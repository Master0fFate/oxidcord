# Local Status Badge

Registers a `status-bar` item with the current channel id, a badge and a button that cycles the badge through three colors. It shows how `current-channel`, channel switch events and `on-ui-action` slot replacement fit together: the button returns a new tree and the host swaps it into the slot.

Permissions: `context-basic`, `ui-slots`.
