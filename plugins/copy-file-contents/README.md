# Copy File Contents

Adds a copy icon button next to text attachments. Clicking it downloads the file, decodes it as UTF-8 and puts the text on the clipboard.

A text attachment is one with a content type starting with `text/` or a common code or config extension (txt, md, json, toml, yaml, xml, csv, log, js, ts, py, rs, go, java, c, cpp, cs, html, css, sh and a few more), and a size under 512 KiB.

Limits: no settings. The clipboard accepts 64 KB, longer text is cut at that size and a toast says so. Invalid UTF-8 bytes become the replacement character. Downloads are limited to 10 per minute. The button needs the message data, so it does not show in direct messages (that needs `context-private`, which this plugin does not ask for).

Permissions: `ui-mount`, `context-timeline`, `context-attachments`, `actions-clipboard`.

Port of the Vencord plugin copyFileContents.
