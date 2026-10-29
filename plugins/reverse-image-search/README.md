# Reverse Image Search

Adds a Search image submenu to the menu of an image in a message with five engines: Google Lens, Yandex, TinEye, SauceNAO and Bing. Picking one opens the engine page for the image link in the browser.

Limits: no settings. The host allows `open-url` once per 10 seconds, so the All entry of Vencord (which opens every engine at once) is left out, and a second search within 10 seconds shows a notice. Vencord also has IQDB and ImgOps, they are not included.

Permissions: `ui-mount`, `actions-open-url`.

Port of the Vencord plugin reverseImageSearch.
