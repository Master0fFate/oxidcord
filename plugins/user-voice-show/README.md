# User Voice Show

Shows a speaker badge next to the name of every user who is in the voice call you are in. The tooltip names the voice channel.

The badge list is checked when the call changes and every ten seconds, so it also goes away when you leave the call.

Settings:
- `showInUserProfileModal`, `showInMemberList`, `showInMessages`: Vencord places the badge in three spots. Oxidcord draws the badges of a plugin in all the places it supports at once, so the three only decide together whether the badge is emitted: all three off turns it off.

Limits: only the call you are connected to is known. The client does not expose the voice states of other voice channels or other servers, and the member information offered to plugins carries none, so people in channels you are not in get no badge. Only one display plugin can decorate a user, if another enabled plugin returns an override for the same user first, this one is skipped for that user.

Permissions: `user-display`, `context-voice`, `storage`.

Port of the Vencord plugin userVoiceShow.
