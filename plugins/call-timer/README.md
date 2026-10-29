# Call Timer

Shows how long the current call has lasted in the voice panel. The timer runs once a second, only while you are in a call, and stops by itself when you leave.

Settings:
- `format`: `stopwatch` (`1:02:03`, with a day count after 24 hours) or `human` (`1h 2m 3s`). Default `stopwatch`.

Limitations: Vencord accepts any moment.js format, here there are the two it offers. The time counts from the moment the client connected to the call.

Permissions: `context-voice`, `ui-mount`, `storage`.

Port of the Vencord plugin callTimer.
