# More Quick Reactions

Adds a row of reaction buttons to the message hover bar. Clicking one adds that reaction to the message.

The row shows the emoji you react with most, filled up with a default set (thumbs up, heart, joy, open mouth, cry, tada, fire, eyes, 100, pray, check mark, heart eyes). Every reaction you add, from this row or anywhere else, counts, and the row reorders itself. Counts are kept for the 40 most used emoji.

Settings: `reactionCount`, from 0 to 12, default 4.

Limits: the Vencord plugin changes the length of Discord's own quick react list, this one adds its own row next to it, and its maximum is 12 instead of 42 so the hover bar still fits. Custom emoji show as `:name:` text on the button because buttons cannot hold images. The client does not expose your favourite emoji yet, so only usage counts decide the order. Reactions in direct messages are not counted (no `context-private`).

Permissions: `ui-mount`, `actions-react`, `storage`, `context-timeline`, `context-basic`.

Port of the Vencord plugin moreQuickReactions.
