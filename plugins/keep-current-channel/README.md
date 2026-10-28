# Keep Current Channel

Remembers the channel you opened last and reopens it when the plugin starts and again when the gateway connects, so a restart lands where you left off.

For 15 seconds after the plugin starts, channel switches that are not the remembered channel (the one the app opens by itself) are not saved over it. Direct messages are remembered as well.

Limits: the Vencord plugin also restores the channel after switching accounts. Oxidcord has one account per session, so only restarts are covered. A channel that no longer exists is skipped without an error.

Permissions: `context-basic`, `storage`, `actions-navigate`.

Port of the Vencord plugin keepCurrentChannel.
