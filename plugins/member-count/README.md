# Member Count

Shows the online count, the total member count and the people in your voice call as a row of small icons above the member list, and the online and total counts in the tooltip of the server icon.

The row is refreshed every 30 seconds and when the voice call changes.

Settings:
- `toolTip`: counts in the server tooltip. Default on.
- `memberList`: counts above the member list. Default on.
- `voiceActivity`: add the voice count to the member list row. Default on.

Limits: the online count is the number of members with a status other than offline among the first 1000 members of the member list the client has loaded, so it can be lower than the real number, and a trailing `+` marks a capped list. The server tooltip only has it for the server whose member list is loaded, other servers show `?`. The voice count is the number of people in the call you are in, and only when that call is in this server. Vencord counts everyone in the voice channels of the server, the client does not expose that. The row is only shown when the client knows the total member count of the server.

Permissions: `ui-mount`, `context-directory`, `context-voice`, `storage`.

Port of the Vencord plugin memberCount.
