# Valid User

Fixes mentions of users the client does not know, which show up as a bare number. The plugin spots such a mention while a message is drawn, loads the user and redraws the message, after which the mention shows the name.

This runs for every message you see, not only on hover like Vencord. Loads are paced: at most two users per step, the host limits the lookups further and a rate limited step waits five seconds. A user that cannot be loaded is not asked for again until the plugin restarts.

Limits: the mention keeps the client's own mention node (click for the profile, right click for the menu) and shows the name because the loaded user is now known to the client, the plugin does not rewrite the mention text. Role mentions and mentions in embeds are not touched.

Permissions: `message-intercept`, `context-members`.

Port of the Vencord plugin validUser.
