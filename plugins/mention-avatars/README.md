# Mention Avatars

Puts the avatar of the mentioned user in front of every user mention in a message. The avatar and the name come from users the client already knows, mentions of unknown users stay as they are.

Settings: `showAtSymbol` (default on). Turn it off and the mention is drawn as the name without the `@`, in the mention colors.

The markdown tree plugins get from `render-message` has a user mention node but no way to hold an image, so the port works in `intercept-message`, which can return inline images.

Limits:
- Role icons in role mentions are not added.
- With `showAtSymbol` off the mention is plain styled text and no longer opens the profile on click.
- Avatars are square, the plugin API cannot round them.
- Messages in direct messages are not touched because the plugin does not ask for `context-private`.

Permissions: `message-intercept`, `context-members`, `storage`.

Port of the Vencord plugin mentionAvatars.
