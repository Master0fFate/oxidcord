# Super Reaction Tweaks

Adds Super react to the menu of a reaction on a message: it adds your own super reaction with that emoji. The client also makes the reaction picker add super reactions by default while the plugin is on.

Settings:
- `superReactByDefault`: the reaction picker adds super reactions instead of normal ones. Default on. Turning the plugin off puts the picker back.
- `unlimitedSuperReactionPlaying` and `superReactionPlayingLimit`: kept so the settings keep their place, they are not applicable, the client has no limit on playing super reactions that a plugin could change.

Limits: Discord only accepts super reactions from accounts that can use them, the note on the failed click is the answer of the client.

Permissions: `ui-mount`, `actions-react`, `actions-settings`, `storage`.

Port of the Vencord plugin superReactionTweaks.
