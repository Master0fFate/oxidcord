# Notes Panel

Local notes with their own page.

- A notebook icon in the server rail (`app-nav`) opens the notes page (`main-view`). The page has three tabs: the note list with a delete button per note, an Add tab with a form (title and text, submitted with Add note or Enter in the title field), and an About tab.
- Right click a message and pick Save to notes to store its text as a note. The item shows only for messages with text.
- A status bar item shows the number of notes and how long ago the last one was saved. A timer refreshes it every 30 seconds.

Notes live in plugin storage under `note.<id>`, at most 200.

Permissions: `ui-mount` (rail icon, page, menu item, status bar item), `storage` (keeps the notes), `context-timeline` (reads the message you save).

Plugin API 1.1, built with the SDK feature `ext`.
