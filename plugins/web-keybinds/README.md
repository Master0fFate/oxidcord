# Web Keybinds

Adds the keybinds the web version of Discord is missing: Alt+Left goes back and Alt+Right goes forward. With `showNavigationButtons` (default on) the title bar also shows back and forward buttons.

Limitations: `overrideCommonKeybinds` is dropped, it only unblocks browser shortcuts like Ctrl+T and has no meaning in a native client. Ctrl+comma and Cmd+comma are not bound, the client opens the settings with them natively and plugins cannot take them over.

Permissions: `ui-keybinds`, `actions-navigate`, `ui-mount`, `storage`.

Port of the Vencord plugin webKeybinds.
