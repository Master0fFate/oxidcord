# Decor

Shows the avatar decorations people picked on decor.fieryflames.dev as a ring over their avatar, in messages, the member list and the profile popout.

Every person the plugin sees without a recent answer is queued. About once a second the plugin asks the Decor API for up to 40 of them at once, keeps the answers for four hours in its storage (so a restart does not ask again) and refreshes the people who have a decoration. People without a decoration are remembered too, so they are not asked again before the four hours are over. Bots are skipped.

Settings: none, Decor has no settings except the ones of its decoration picker.

Limits: this only shows decorations. Creating, uploading, choosing or deleting your own decoration is not possible, use the Decor website or Vencord for that. The animation of animated decorations follows the client. The plugin needs the host approvals for decor.fieryflames.dev (the API) and ugc.decor.fieryflames.dev (the decoration images).

Permissions: `user-display`, `net-fetch`, `storage`.

Port of the Vencord plugin decor.
