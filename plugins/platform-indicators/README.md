# Platform Indicators

Adds a small icon for every platform a user is online on, next to their name: a monitor for desktop, a phone for mobile, a globe for web and a gamepad for console or game clients. The icon is colored by the user's status (green online, yellow idle, red do not disturb, grey otherwise) and the tooltip names the platform. Bots get no icon.

The badge is refreshed when the user's status or platforms change.

Settings:
- `show`: show the icons. Default on. Oxidcord draws the badges of a plugin in all the places it supports at once, so Vencord's three settings for the member list, profiles and messages are one switch here.
- `colorMobileIndicator`: color the mobile icon by status like the others. When off the mobile icon keeps the default color. Default on.

Limits: the client only reports desktop, mobile and web per user, the embedded and VR platforms of Vencord appear only when the client reports them. Display overrides of several plugins merge per field: the first plugin in priority order that sets a field wins that field, and badges of all plugins are listed together.

Permissions: `user-display`, `context-presence`, `storage`.

Port of the Vencord plugin platformIndicators.
