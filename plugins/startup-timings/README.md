# Startup Timings

Adds a Startup timings page to the plugin pages in the settings. It lists the phases of the client start (launch, gateway connect, ready and so on) with their offset in milliseconds from launch, and how long the client has been running.

Settings: none. The page refreshes by itself five seconds after the plugin starts and with its Refresh button.

Limitations: Vencord lists the load time of every Vencord plugin, the client only records its own startup phases. The page does not refresh on its own after the first time.

Permissions: `context-basic`, `ui-mount`.

Port of the Vencord plugin startupTimings.
