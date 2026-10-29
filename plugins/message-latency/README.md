# Message Latency

Adds a line under a message that took a while to send, like `Sent with a delay of 3 seconds`. The line is yellow, red when the delay is at least twice the threshold and grey when it is two minutes or more.

Settings: `latency` (threshold in seconds, default 2) and `showMillis` (default off, otherwise the delay is rounded to whole seconds).

Limits: the delay of your own messages is the time between pressing send and the message coming back from the server. For messages from other people it is the difference between the message id and the nonce, like Vencord, so it exists for live messages only and never for history loaded later. Vencord's `detectDiscordKotlin` and the "clock is ahead" text are not part of this port.

Permissions: `message-intercept`, `storage`.

Port of the Vencord plugin messageLatency.
