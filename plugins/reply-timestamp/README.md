# Reply Timestamp

Shows the time of the replied message at the end of the reply quote, like `[3:45 PM]`. When the replied message is from another day than the reply, the date comes first, like `[2026-10-27 3:45 PM]`.

The time is read from the id of the replied message, so it works for every reply, loaded or not.

Settings: `utcOffset` (default `+00:00`).

Limits: plugins cannot read the system time zone, so times are shown in `utcOffset`. Set it to your own offset (for example `+02:00`) once. Daylight saving changes are not followed. The time uses the 12 hour format and is plain text, so it does not follow the 24 hour setting of the client. Vencord writes "Yesterday at" style dates for older replies, this port writes the date. The note is part of the reply quote, so nothing shows for replies the client shows as deleted.

Permissions: `message-intercept`, `storage`.

Port of the Vencord plugin replyTimestamp.
