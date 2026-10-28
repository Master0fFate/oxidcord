# Who Reacted

Shows who reacted to a message.

- A Reactors button on the message toolbar loads the reacting users of every reaction of that message (up to 10 per emoji) and shows them under the message, one row per emoji with a `+N` for the rest.
- The Who reacted item in the menu of a reaction loads only that emoji.
- Loaded users are kept for the last 100 messages you looked at. A Hide link under the rows removes them.

Settings: `clickableAvatars` (default on) shows the users as chips that open their profile when clicked. Off, they are plain avatars.

Limits: Vencord loads the avatars by itself for every reaction. This port only loads on click because every load is a request and the host allows one every 2 seconds after a burst of 5. When a message has more reactions than that, the plugin shows what it got and a toast says to click again for the rest. Rows do not follow new reactions, click again after hiding to refresh. Reactions on direct messages need `context-private`, which this plugin does not ask for.

Permissions: `ui-mount`, `context-timeline`, `message-intercept`, `storage`.

Port of the Vencord plugin whoReacted.
