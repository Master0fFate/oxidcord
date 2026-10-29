# Message Click Actions

Double click a message to edit it (your messages) or reply to it (everyone else's). Ctrl+Alt+click one of your messages to delete it when `enableDeleteOnClick` is on.

Settings: `enableDoubleClickToEdit`, `enableDoubleClickToReply` (default on) and `enableDeleteOnClick` and `requireModifier` (default off). With `requireModifier` on, the double click actions only run while Shift or Ctrl is held.

Reply pings: a plain double click replies with the mention on. Shift+double click replies without the mention, except with `requireModifier` on, where Shift only unlocks the action and the mention stays on (same as Vencord).

Limits:
- Vencord deletes on Backspace+click. A plugin cannot see which keys are held, only the modifiers of a click, so the delete gesture is Ctrl+Alt+click and works on your own messages only. It is off by default, because it deletes at once and Alt+click marks a message unread in Discord. `requireModifier` does not change it.
- Clicks that land on the author, a spoiler, an attachment or an embed are ignored, only the message text and its empty space count.
- There is no check for the permission to send messages in the channel, the composer decides.
- A plugin keeps one click watch per modifier combination, so the gestures above use the four combinations none, Shift, Ctrl and Ctrl+Alt.

Permissions: `ui-input`, `context-timeline`, `actions-compose`, `actions-delete`, `storage`.

Port of the Vencord plugin messageClickActions.
