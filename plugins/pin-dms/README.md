# Pin DMs

Pins DMs and group chats into sections at the top of the DM list. Right click a DM and pick Pin DM, then the category. Unpin DM removes it again.

A first category named Pinned always exists. The Pinned DMs page in the plugin pages of the settings adds and deletes categories and adds or removes DMs in them (the list offers the 50 most recent DMs that are not pinned).

Settings:
- `pinOrder`: `mostRecent` sorts each section by the latest message, `custom` keeps the order you set with Move pin up and Move pin down in the DM menu. Default `mostRecent`.
- `canCollapseDmSection` and `dmSectionCollapsed`: when both are on the pinned sections start collapsed. Default off.

Limitations: partial port.
- The DM menu offers Unpin DM, Move pin up and Move pin down on every DM, they do nothing on a DM that is not pinned. Move pin only shows with `pinOrder` custom.
- The client always lets you fold a section by clicking its header, `canCollapseDmSection` only decides the starting state. The list of DMs that are not pinned cannot be collapsed.
- Categories have no colour and no rename. At most 8 categories, 200 pinned DMs in total.
- `mostRecent` reorders about a second after a pinned DM gets a message, ordering is by message id.
- Vencord keeps categories per account inside its settings, here they live in the plugin storage of this install.

Permissions: `ui-mount`, `context-directory`, `context-private`, `context-timeline`, `actions-settings`, `storage`.

Port of the Vencord plugin pinDms.
