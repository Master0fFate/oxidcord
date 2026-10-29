# User Voice Show

Shows a speaker badge next to the name of every user who is in the voice call you are in. The tooltip names the voice channel.

The badge list is checked when the call changes and every ten seconds, so it also goes away when you leave the call.

Settings:
- `show`: show the badge. Default on. Oxidcord draws the badges of a plugin in all the places it supports at once, so Vencord's three settings for the profile, member list and messages are one switch here.

Limits: only the call you are connected to is known. The client does not expose the voice states of other voice channels or other servers, and the member information offered to plugins carries none, so people in channels you are not in get no badge. Display overrides of several plugins merge per field: the first plugin in priority order that sets a field wins that field, and badges of all plugins are listed together.

Permissions: `user-display`, `context-voice`, `storage`.

Port of the Vencord plugin userVoiceShow.
