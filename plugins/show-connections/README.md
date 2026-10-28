# Show Connections

Adds a Connections section to the profile popout that lists the connected accounts of the person: an icon for the kind of account, the name and a check mark when the account is verified. Accounts that come with a link show the name as a link, the others get a button that copies the name.

The popout only reads profiles the client already has. When the profile is not loaded yet the section shows a Load button, and the click loads the profile and fills the section in.

Settings:
- `iconSize`: icon size in pixels, 16 to 64. Default 32.
- `iconSpacing`: `compact`, `cozy` (default) or `roomy`, the gap between the icon and the name.

Limits: the icons come from a small built in map of Lucide icons (brand logos are not available), unknown kinds get a link icon. The client does not give a link for any connection yet, so every name is copied rather than opened until it does.

Permissions: `ui-mount`, `context-profiles`, `actions-clipboard`, `storage`.

Port of the Vencord plugin showConnections.
