# Account Server Profile

Shows your profile in the server you have open: your avatar, nickname, roles and the day you joined.

- A small row above your account panel shows your name and a button that opens the window.
- Right click the account panel and pick Server profile for the same window.

In direct messages and without a server the window is replaced by a short note.

Settings:
- `prioritizeServerProfile`: the row above the account panel shows your server nickname instead of your account name while a server is open. Default off.

Limits: the client's own account popout is not replaced, so a left click on the account panel still opens the account profile and `prioritizeServerProfile` only changes the name in the row. The window has your server data only (avatar, nickname, roles, join date), not the banner, bio or pronouns. The name follows server changes, it does not follow a nickname you change while the server stays open.

Permissions: `ui-mount`, `context-basic`, `context-roles`, `storage`.

Port of the Vencord plugin accountPanelServerProfile.
