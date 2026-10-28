# Mutual Group DMs

Adds a Mutual groups section to the profile popout with the group chats you share with that user. Each row shows the group name and its member count, clicking the name opens the chat. The section is left out when you share no group chat with the user, and for yourself.

The list is rebuilt from the group chats the client has loaded, and dropped when a group chat is closed or the channel list changes.

Limits: at most 30 groups are listed, group icons are not shown, and the section is only in the profile popout, not in the full profile window or the direct message side profile. The member count is the number of recipients plus you.

Permissions: `ui-mount`, `context-directory`, `context-private`, `actions-navigate`.

Port of the Vencord plugin mutualGroupDMs.
