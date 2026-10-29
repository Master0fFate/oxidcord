# New Guild Settings

When you join a server, applies your default notification settings to it.

Settings:
- `guild`: mute the server (default on).
- `messages`: message notifications, `all`, `mentions`, `nothing` or `default` to leave the server default alone (default).
- `everyone`: suppress @everyone and @here (default on).
- `role`: suppress all role mentions (default on).
- `highlights`, `events`, `showAllChannels`: kept so your settings carry over, but not applied.

Limits: the client has no setting for suppressing highlights, muting scheduled events or showing all channels of a community server, so those three do nothing. The settings are applied once when the join is seen, not to servers you are already in. The Apply NewGuildSettings menu entry of Vencord is not included.

Permissions: `context-directory`, `actions-settings`, `storage`.

Port of the Vencord plugin newGuildSettings.
