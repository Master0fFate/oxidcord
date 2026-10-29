# Super Reaction Tweaks

Adds Super react to the menu of a reaction on a message: it adds your own super reaction with that emoji. The client also makes the reaction picker add super reactions by default while the plugin is on.

Settings:
- `superReactByDefault`: the reaction picker adds super reactions instead of normal ones. Default on. Turning the plugin off puts the picker back.

Limits: Vencord's settings for the super reaction playing limit are not part of this port, the client has no such limit. Discord only accepts super reactions from accounts that can use them, the note on the failed click is the answer of the client.

Permissions: `ui-mount`, `actions-react`, `actions-settings`, `storage`.

Port of the Vencord plugin superReactionTweaks.
