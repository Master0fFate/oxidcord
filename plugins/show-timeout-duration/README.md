# Show Timeout Duration

Adds a clock badge to members who are timed out and shows how much longer the timeout lasts, like `2h 10m left`.

Settings: `displayStyle` (`inline` by default, or `tooltip`). Inline writes the remaining time next to the name and the badge tooltip says `Timed out`. Tooltip keeps the name clean and puts `2h 10m timeout remaining` in the tooltip of the badge.

The time is recounted once a minute while a timed out member is on screen, so it only shows whole minutes (`<1m` for the last minute). A badge disappears once the timeout ends.

Limits: Vencord changes the tooltip of the timeout icon in the message header with a live countdown. A plugin cannot reach that icon, so this port adds its own badge to every place a name override shows (messages, member list, profile popout). Only members the client has loaded can be checked.

Permissions: `user-display`, `context-roles`, `storage`.

Port of the Vencord plugin showTimeoutDuration.
