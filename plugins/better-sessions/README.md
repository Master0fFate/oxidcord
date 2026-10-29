# Better Sessions

Lists the devices logged in to your account on the settings page of the plugin.

- Each session shows its system, client, location and the exact time it was last used, with a This device mark for the current one.
- A name field and Save give a session your own name. Saving an empty name removes it. Names are kept per session id.
- A new session id triggers a notification with its system, client and location. The first run only remembers the sessions that exist and stays quiet.

Settings:
- `backgroundCheck`: also look for new sessions on a timer (default off). A check always runs shortly after the client starts.
- `checkInterval`: minutes between background checks (default 20, 1 to 1440).

Limits: the list comes from the client's own copy, which the client refreshes about once a minute, so a new session can show up a little late. The Refresh button asks again. Vencord changes Discord's own devices screen, this port has its own list on the plugin page and has no New badge. Sessions cannot be logged out from here.

Permissions: `context-sessions`, `ui-mount`, `actions-notify`, `storage`.

Port of the Vencord plugin betterSessions.
