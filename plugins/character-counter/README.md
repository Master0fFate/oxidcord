# Character Counter

Shows how many characters the message you are typing has, as `count/limit` in the status bar under the composer. The counter is hidden while the draft is empty and only redraws when the count changes. Characters are counted like Discord does, in UTF-16 units.

Settings: `colorEffects` (default on) tints the counter warning color from 50 percent of the limit and danger color from 90 percent. `limit` (default 2000) is the message length limit; set it to 4000 with Nitro.

Limitations: plugins cannot see whether the account has Nitro, so the limit is a setting. The selected-character count of Vencord is not shown, plugins cannot see the text selection. Colors are the client theme tokens (muted, warning, danger) instead of the yellow, orange and red steps.

Permissions: `composer-watch`, `ui-mount`, `storage`.

Port of the Vencord plugin characterCounter.
