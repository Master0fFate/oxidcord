# Message Latency

Adds a line under a message that took a while to send, like `Sent with a delay of 3 seconds`. The line is yellow, red when the delay is at least twice the threshold and grey when it is two minutes or more.

Settings: `latency` (threshold in seconds, default 2), `showMillis` (default off, otherwise the delay is rounded to whole seconds), `ignoreSelf` (default off).

Limits: the client only measures the delay of messages you sent yourself (time between pressing send and the message coming back from the server). Vencord compares the message id with the nonce, which is not available for messages from other people, so no line ever shows for them. Because of that `ignoreSelf` hides every line when it is on. Vencord's `detectDiscordKotlin` and the "clock is ahead" text are not part of this port.

Permissions: `message-intercept`, `storage`.

Port of the Vencord plugin messageLatency.
