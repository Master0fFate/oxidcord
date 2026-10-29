# Voice Chat Double Click

Joins a voice or stage channel on a double click instead of a single click. A single click on a voice channel does nothing, unless you are already in it, then it opens the channel like any other.

Settings: none.

Limitations: the client consumes every plain click (no modifier keys held) on a channel row while the plugin is on, and the plugin opens text channels and threads itself with a normal channel switch. Clicks with ctrl, shift, alt or meta held, and middle clicks, keep their normal behaviour. Plugins cannot watch voice channels only, so the cost is one plugin call per channel click.

Permissions: `ui-input`, `context-directory`, `context-voice`, `actions-voice`, `actions-navigate`.

Port of the Vencord plugin vcDoubleClick.
