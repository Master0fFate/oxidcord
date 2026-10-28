# Implicit Relationships

Adds an Implicit tab to the Friends page with the people you share a server with but have no relationship with, sorted by name. Each row shows the avatar, the display name and the username.

The list is read from the client and refreshed when a relationship changes and every 60 seconds. At most 300 people are drawn, the line at the top shows the full count.

Limits: Oxidcord has no affinity data from Discord, it treats users who are in two or more of your loaded servers as implicit. That is why the Vencord setting `sortByAffinity` is not part of this port, the order is always by name. The tab is its own page, the core Friends list and its counters are not changed.

Permissions: `ui-mount`, `context-relationships`.

Port of the Vencord plugin implicitRelationships.
