# Override Forum Defaults

Chooses the layout and sort order forums open with. You can still change both per forum while it is open.

Settings:
- `defaultLayout`: `list` or `gallery`. Default `list`.
- `defaultSortOrder`: `recent` (recently active) or `posted` (date posted). Default `recent`.

Limitations: Vencord stores the choices as numbers, here they are words. The client applies the defaults when a forum is opened and reverts them when the plugin is turned off.

Permissions: `actions-settings`, `storage`.

Port of the Vencord plugin overrideForumDefaults.
