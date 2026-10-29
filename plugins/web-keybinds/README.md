# Web Keybinds

Adds the keybinds the web version of Discord is missing: Ctrl+comma opens the settings on the account page, Alt+Left goes back and Alt+Right goes forward. With `showNavigationButtons` (default on) the title bar also shows back and forward buttons.

Limitations: `overrideCommonKeybinds` is dropped, it only unblocks browser shortcuts like Ctrl+T and has no meaning in a native client. Ctrl+comma already opens the settings in the client, here it opens the account page. Cmd+comma is not bound on macOS, the client has it natively.

Permissions: `ui-keybinds`, `actions-navigate`, `ui-mount`, `storage`.

Port of the Vencord plugin webKeybinds.
