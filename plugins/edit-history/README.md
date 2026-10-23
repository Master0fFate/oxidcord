# Edit History

Remembers the text of the newest 500 messages it sees (cut to 2000 characters). When one of them is edited, the previous text is added to that message's history: up to 10 versions per message, oldest dropped first. Under an edited message it adds a line `edited N times` followed by the latest previous versions as quotes (each cut to 200 characters). Messages without history are left untouched.

Settings: `show_versions` (off shows only the count line) and `version_count` (1 to 10, default 3).

Storage: messages are packed 25 to a key and histories into 16 keys, so it stays far below the 1024 key limit. Versions are kept to 600 characters, which bounds the worst case under the 5 MB limit. A message that leaves the 500 message window loses its history. Edits to messages the plugin never saw (sent before it started) have no earlier text to keep.

Direct messages are not tracked: the plugin does not request `context-private`, so the host never delivers those events.

Permissions: `context-timeline`, `storage`, `message-render`.
