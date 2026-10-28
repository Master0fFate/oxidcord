# View Icons

Adds image entries to two context menus.

- Member menu: View avatar and View banner. The banner comes from the profile, if the client has not loaded it yet the click loads it.
- Server menu: View icon and View banner.

Each entry opens a window with the image at the chosen size and format and an Open link to the original (size 4096). When the person or server has no such image a short note shows instead.

Settings:
- `format`: `webp` (default), `png` or `jpg` for images that are not animated. Animated images always use webp.
- `imgSize`: `128`, `256`, `512`, `1024` (default), `2048` or `4096`.

Limits: clicking the avatar or banner inside the profile popout is not supported, and neither are the server avatar, the avatar decoration and group chat icons. The window draws the image at most 512 pixels wide, the size setting only changes which file is loaded and what Open points to.

Permissions: `ui-mount`, `context-profiles`, `context-directory`, `storage`.

Port of the Vencord plugin viewIcons.
