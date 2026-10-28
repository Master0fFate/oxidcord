# Send Timestamps

Adds a calendar button to the composer. It opens a picker with a date, a time, a UTC offset and a style. The picker shows a preview and `Insert` puts the Discord timestamp token (`<t:1700000000:F>`) into the composer. Styles: default, `t`, `T`, `d`, `D`, `f`, `F`, `s`, `S` and `R`.

With `replaceMessageContents` on, a time in backticks is turned into a timestamp when you send: `` `17:30` `` or `` `5:30 PM` `` becomes `<t:...:t>` for the next time that clock reads that. The composer shows its usual indicator that a plugin changed the message. Invalid times such as `` `24:00` `` are left as they are.

Settings: `replaceMessageContents` (default on) and `utcOffset` (default `+00:00`).

Limits: plugins cannot read the system time zone, so the picker and the backtick shortcut read the time in `utcOffset`. Set it to your own offset (for example `+02:00`) once, the picker also lets you change it for a single timestamp. Daylight saving changes are not followed. The preview draws `s` and `S` like `f`, the inserted token is still correct.

Permissions: `ui-mount`, `composer`, `message-send`, `storage`.

Port of the Vencord plugin sendTimestamps.
