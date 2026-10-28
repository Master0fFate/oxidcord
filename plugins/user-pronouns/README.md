# User Pronouns

Shows the pronouns from a user's profile next to their name in messages, the member list and the profile popout.

The client only reads profiles it already has. Every author the plugin sees without a loaded profile is queued and the plugin loads one profile every 2 seconds from a timer, so pronouns fill in over time while you scroll and chat. Bots are skipped. The memory of loaded profiles is cleared when the client reconnects.

Settings:
- `pronounsFormat`: `lowercase` (default) or `capitalized`, which keeps the casing the user wrote.
- `showSelf`: show your own pronouns. Default on.

Limits: global profile pronouns only, per server pronouns are not exposed by the client. Profiles load one at a time, so a busy channel takes a while to fill in. Only one display plugin can set pronouns for a user, the first plugin in priority order wins the field.

Permissions: `user-display`, `context-profiles`, `context-basic`, `storage`.

Port of the Vencord plugin userMessagesPronouns.
