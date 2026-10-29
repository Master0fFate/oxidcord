# Porting Vencord plugins

Oxidcord does not run Vencord plugins. They are written for the Discord web client and patch its JavaScript. Where a plugin's idea fits the Oxidcord plugin API, it was rewritten from scratch as a WebAssembly plugin: Vencord's source was read for behaviour and setting names only. The ports live in `plugins/`, see `plugins/README.md` for what each one does.

The table covers all 168 plugins in the `src/plugins` folder of the Vencord repository, except the internal `_api` and `_core` folders.

| Status | Count | Meaning |
| --- | --- | --- |
| ported | 21 | Does what the Vencord plugin does, with the same setting keys. Notes list small differences |
| partial | 78 | The main feature works, parts are left out. Notes list what |
| not ported | 69 | No Oxidcord plugin. 39 are impossible, 11 are ruled out by policy and 19 need a capability the API does not have |

99 of 168 plugins have an Oxidcord counterpart, 95 of them added with API 1.2. Four were written earlier: `fake-nitro`, `message-logger`, `see-hidden-channels` and `silent-type`.

Reasons for plugins that are not ported:

- **impossible**: the plugin changes something that only exists in the Discord web client (webpack modules, DOM, CSS, the console, the browser), or that Oxidcord already does natively.
- **policy**: the plugin would break a rule of the client. Oxidcord presents as the Discord web client and does not bypass client checks, spoof Nitro or Spotify Premium, set rich presence or activities for plugins, automate user actions, or touch captchas, MFA or rate limits.
- **missing capability**: the plugin could be written if the API had a function or hook it lacks, for example a GIF picker hook, microphone capture, local sockets or raw REST access. Plugins never get raw REST access: every write maps to a typed session command.

Notes for ports describe what is different from the Vencord plugin. Settings keep Vencord's keys and defaults unless a note says otherwise. Most ports only handle servers because they do not hold `context-private`; a note says "no DMs" where that matters.

| Vencord plugin | Oxidcord plugin | Status | Notes |
| --- | --- | --- | --- |
| `accountPanelServerProfile` | `account-server-profile` | partial | The client's account popout is not replaced: a row above the account panel and a menu entry open a window with avatar, nickname, roles and join date, prioritizeServerProfile only switches the row to the server nickname. |
| `addAttachments` | | not ported | Missing capability: adding files to an existing message (no attachment edit). |
| `alwaysAnimate` | | not ported | Impossible: rewrites Discord render props that Oxidcord does not have. |
| `alwaysExpandRoles` | | not ported | Impossible: patches Discord popout component state. |
| `alwaysTrust` | | not ported | Policy: removes the link and file safety prompt, a client check bypass. |
| `anonymiseFileNames` | `anonymise-file-names` | partial | One composer toolbar toggle for the next send instead of a per-file toggle in the attachment bar. |
| `arRPC.web` | | not ported | Policy: no plugin-set rich presence, and no local RPC socket for plugins. |
| `autoDndWhilePlaying.discordDesktop` | | not ported | Policy: needs game detection and activity handling, the client sends no activities. |
| `betterFolders` | `better-folders` | partial | Sidebar maps to folder-sidebar and keepIcons to plain-folder-icons inverted, close and force open rules cannot be driven (folder open state is not exposed), sidebarAnim, showFolderIcon and the close settings are kept but do nothing, it also overrides Plain Folder Icon while the sidebar is on. |
| `betterGifAltText` | `better-gif-alt-text` | partial | Alt is not supported on embeds so a GIF embed is replaced by a card with the alt as title and a still thumbnail (animation lost, thumbnail needs approval of the tenor, giphy and imgur hosts). |
| `betterGifPicker` | | not ported | Missing capability: GIF picker hook. |
| `betterRoleContext` | `better-role-context` | partial | Edit Role and View Role Members dropped, entries show on every role and answer with a notice when the role has no color or icon. |
| `betterRoleDot` | `better-role-dot` | partial | Client badge (circle icon) instead of Discord's role dot, bothStyles only sets the name color, copyRoleColorInProfilePopout has no separate effect (click always copies), user in several servers copies the last color drawn. |
| `betterSessions` | `better-sessions` | partial | Own list on the plugin settings page instead of Discord's devices screen, no NEW badge, first run only remembers existing sessions silently, list depends on the client's session refresh (about once a minute). |
| `betterSettings` | | not ported | Impossible: patches Discord's settings modal. |
| `betterUploadButton` | | not ported | Missing capability: composer upload button override. |
| `biggerStreamPreview` | | not ported | Missing capability: Go Live stream previews, the client has no Go Live support. |
| `blurNsfw` | `blur-nsfw` | partial | blurAmount accepted but ignored (host blur is fixed), attachments only, 10 per message. |
| `callTimer` | `call-timer` | ported | Only the stopwatch and human formats (no moment.js format), 1 s timer runs only during a call and stops itself when voice-detail is gone. |
| `characterCounter` | `character-counter` | partial | Limit is a setting (2000 or 4000) because the user record has no premium flag, no selected-character count, theme tone tokens instead of yellow/orange/red. |
| `clearURLs` | `clear-urls` | ported | Uses regex-lite so a rule with look-around would be skipped (none today); completeProvider entries are ignored; links inside code blocks are cleaned too, message edits are not cleaned. |
| `clientTheme` | | not ported | Impossible: CSS injection, themes are handled by the core theme system. |
| `colorSighted` | | not ported | Impossible: Discord's internal status mask icons. |
| `consoleJanitor` | | not ported | Impossible: web console. |
| `consoleShortcuts` | | not ported | Impossible: window globals of the web client. |
| `copyEmojiMarkdown` | `copy-emoji-markdown` | partial | Unicode emoji have no shortcode in the API so copyUnicode off copies nothing and shows a notice. |
| `copyFileContents` | `copy-file-contents` | ported | Button beside text attachments under 512 KiB only (not inside the file preview), clipboard cut at 64 KB with a toast, 10 downloads per minute, absent in DMs. |
| `copyStickerLinks` | `copy-sticker-links` | ported | Open link is limited to one per 10 seconds, unknown stickers are linked as png. |
| `copyUserURLs` | `copy-user-urls` | ported | Copies the plain link, Vencord wraps it in angle brackets. |
| `crashHandler` | | not ported | Impossible: Discord's React crash screen. |
| `customCommands` | `custom-commands` | partial | No slash command options so arguments are positional words, no ephemeral option, commands cannot be unregistered so a deleted tag stays in the slash list until restart. |
| `customIdle` | `custom-idle` | partial | Sets status idle after the timeout, does not change Discord's own idle timer, only online is changed, checked every 30 s, remainInIdle uses a toast with a Go online button. |
| `customRPC` | | not ported | Policy: no plugin-set rich presence, only status and custom status. |
| `dearrow` | `dearrow` | partial | The embed becomes a card (title, description, link, provider footer, thumbnail) so the video player and uploader name are lost, thumbnails need approval of dearrow-thumb.ajay.app and i.ytimg.com, lookups paced by the fetch rate limit. |
| `decor` | `decor` | partial | Display only: no creating, uploading or choosing decorations, lookups are batches of 40 every 6.5 s, answers kept 4 hours. |
| `devCompanion.dev` | | not ported | Impossible: webpack developer tooling. |
| `directFileDownload.desktop` | `direct-file-download` | ported | Save dialog of the client on every attachment (button plus Save as menu entry), the built-in download button on files still only opens the browser. |
| `disableCallIdle` | | not ported | Impossible: Discord voice client timer, handled by the core. |
| `dontRoundMyTimestamps` | | not ported | Impossible: rounding is host side with no option. |
| `experiments` | | not ported | Policy: Discord developer flags and experiments. |
| `expressionCloner` | `expression-cloner` | partial | Server list is owned servers plus Administrator, Manage Expressions or Create Expressions read from the first channel, free slot counts unknown so full servers are still listed, only custom emoji and stickers the client already loaded, stickers keep their own name. |
| `f8break` | | not ported | Impossible: browser DevTools. |
| `fakeNitro` | `fake-nitro` | partial | Emoji bypass only. Sticker bypass, Nitro themes and stream quality are not implemented, and DMs are not shown inline without context-private. |
| `fakeProfileThemes` | `fake-profile-themes` | partial | Popout itself is not re-themed (only a colored section with two swatches), no color picker or Copy 3y3 button, nitroFirst treats a profile accent color as the Nitro signal, Load button while the profile is not cached. |
| `favEmojiFirst` | `fav-emoji-first` | partial | List-favorite-emojis is empty so it learns from emoji in sent messages (last 50, storage-backed), reactions and picker picks not learned. |
| `fixCodeblockGap` | | not ported | Impossible: Discord markdown regex. |
| `fixImagesQuality` | `fix-images-quality` | partial | Swaps the attachment url for the original file (strips width, height, format, quality) within the 2000x1200 limit or for all images with originalImagesInChat, host rule prevents building resized proxy urls, no freeze while unfocused, attachments only. |
| `fixSpotifyEmbeds.desktop` | | not ported | Impossible: Discord's embedded Spotify player. |
| `fixYoutubeEmbeds.desktop` | | not ported | Impossible: Electron response rewrite. |
| `forceOwnerCrown` | `force-owner-crown` | partial | Own crown badge next to the name instead of Discord's icon, owner remembered until restart or reconnect. |
| `friendInvites` | | not ported | Missing capability: friend invite endpoints, plugins have no raw REST. |
| `fullSearchContext` | | not ported | Missing capability: search results surface. |
| `fullUserInChatbox` | | not ported | Impossible: composer editor nodes. |
| `gameActivityToggle` | | not ported | Policy: toggles game activity, plugins cannot control activities. |
| `gifPaste` | | not ported | Missing capability: GIF picker select hook. |
| `gifProviderSwitcher` | | not ported | Missing capability: GIF picker hook and provider switch. |
| `greetStickerPicker` | | not ported | Impossible: Discord's welcome message button. |
| `hideAttachments` | `hide-attachments` | partial | Stickers and components stay visible, one toggle button for hide and show, not applied in DMs (no context-private). |
| `ignoreActivities` | | not ported | Policy: filters outbound activities, the client sends none. |
| `iLoveSpam` | | not ported | Impossible: spam classifier hiding is core. |
| `imageFilename` | `image-filename` | partial | Attachments only (embeds cannot carry a tooltip), tooltip cut at 200 bytes, 10 attachments per message. |
| `imageLink` | `image-link` | partial | The message list hides the link text before plugins run, so the link is added as a line under the image instead of render-message; shows twice when link previews are off; no DMs, no video links. |
| `imageZoom` | | not ported | Missing capability: image viewer hook with pointer and scroll events. |
| `implicitRelationships` | `implicit-relationships` | partial | Own Friends tab (the core list and counters are not changed), implicit means 2+ shared loaded servers so sortByAffinity is dropped and the order is by name, 300 rows max. |
| `ircColors` | `irc-colors` | partial | memberListColors dropped because the client applies a name override to messages, member list and popout together; role color check needs context-roles, without it members count as having none. |
| `jumpToTop` | `jump-to-top` | partial | Button also shows in forum posts (cannot be told apart). |
| `keepCurrentChannel` | `keep-current-channel` | ported | Restores after restart and gateway connect only, the account switch restore does not apply to a single account client. |
| `loadingQuotes` | | not ported | Impossible: Discord's loading screen strings. |
| `memberCount` | `member-count` | partial | Online is counted over the loaded member list (capped at 1000, tooltip only for the selected server), voice is only the call you are in. |
| `mentionAvatars` | `mention-avatars` | partial | Works in intercept-message because render-message output cannot hold images (the md tree has a user mention node); role icons not added, avatars are square, showAtSymbol off turns the mention into plain styled text, no DMs. |
| `messageClickActions` | `message-click-actions` | partial | Backspace cannot be detected so delete is Ctrl+Alt+click on your own message and off by default (Alt+click is mark unread in Discord), body clicks only, no send permission check, uses four click watches (none, Shift, Ctrl, Ctrl+Alt). |
| `messageLatency` | `message-latency` | partial | Only your own messages have a measured delay so other people never get a line and ignoreSelf hides all of them, detectDiscordKotlin dropped, latency/showMillis/ignoreSelf keys kept. |
| `messageLinkEmbeds` | `message-link-embeds` | partial | Preview is an embed card with a Jump button in an accessory (first image only, no avatar or reactions), no DM links (no context-private), automodEmbeds maps to a compact one line card, no clear cache button. |
| `messageLogger` | `message-logger` | partial | Deleted messages stay in red and edits get a history button. No ignore lists or style settings, kept in memory and storage only, DMs are not logged. |
| `moreQuickReactions` | `more-quick-reactions` | partial | Adds its own row next to Discord's list instead of lengthening it, default 4 and maximum 12, custom emoji show as :name: text, favourite emoji are not exposed by the client so order follows own reaction counts, DMs not counted. |
| `musicRichPresence` | | not ported | Policy: no plugin-set rich presence. |
| `mutualGroupDMs` | `mutual-group-dms` | partial | Profile popout section only, 30 groups max, no group icons. |
| `newGuildSettings` | `new-guild-settings` | partial | Highlights, events and showAllChannels kept as settings but not applied (no matching setting in the client), no Apply menu entry, messages uses all/mentions/nothing/default. |
| `noBlockedMessages` | `no-blocked-messages` | partial | ignoreMessages dropped (a filter cannot stop unread counts or notifications), ignored users are an ignoredUsers id list because there is no ignore state. |
| `noDeepLinks.web` | | not ported | Impossible: web bundle behaviour. |
| `noDevtoolsWarning` | | not ported | Impossible: browser console banner. |
| `noF1` | | not ported | Impossible: Discord's keybind table. |
| `noMaskedUrlPaste` | | not ported | Impossible: Discord's composer paste handling. |
| `noMiddleClickPaste` | | not ported | Impossible: raw input handling in the core. |
| `noMosaic` | `no-mosaic` | partial | Layout is stacked or inline-video, so with inlineVideo on a message that has a video uses inline-video and its images keep the default layout. |
| `noOnboardingDelay` | | not ported | Impossible: Discord's onboarding flow. |
| `noPendingCount` | | not ported | Missing capability: suppressing the friend and message request counts. |
| `noProfileThemes` | | not ported | Impossible: Oxidcord does not draw Nitro profile themes, there is nothing to turn off. |
| `noReplyMention` | `no-reply-mention` | partial | Rule applied at send so it overrides the reply bar ping toggle, inverseShiftReply not applicable (shift state not visible), roles need the replied message in the cache and are read in servers only. |
| `noServerEmojis` | `no-server-emojis` | partial | A query that matches only server emoji leaves nothing to keep and the empty answer means no opinion so the default list shows, DMs need context-private to be recognised. |
| `noSystemBadge.discordDesktop` | | not ported | Impossible: app shell badge, a core setting. |
| `notificationVolume` | `notification-volume` | ported | Volume applies to the notification chime, the only sound the client plays itself. |
| `noTypingAnimation` | `no-typing-animation` | ported | No known limitations. |
| `noUnblockToJump` | | not ported | Impossible: Discord's jump guard. |
| `oneko` | | not ported | Impossible: DOM script. |
| `onePingPerDM` | `one-ping-per-dm` | ported | Silences the sound only, the notification itself still shows. |
| `openInApp` | `open-in-app` | partial | Epic dropped (no scheme in the allowlist), short links (spotify.link, s.team) not followed, Apple Music opens through music: instead of itunes:, no toast, profile connected-account clicks not covered. |
| `overrideForumDefaults` | `override-forum-defaults` | ported | Layout and sort order are words (list or gallery, recent or posted) instead of numbers, applied when a forum opens. |
| `pauseInvitesForever` | | not ported | Missing capability: server invite settings endpoint, plugins have no raw REST. |
| `permissionFreeWill` | | not ported | Policy: bypasses client side permission checks. |
| `permissionsViewer` | `permissions-viewer` | partial | Final permission bits only (no per role or override breakdown, no channel overwrites list), user view reads the open channel or the first channel of the server, no popout button, permissionsSortOrder orders roles. |
| `petpet` | `petpet` | partial | User mention or id and delay only (no image attachment, url, resolution or server avatar), fixed 128 px and 10 frames, hand drawn procedurally, avatar download limited to 10 per minute. |
| `pictureInPicture` | | not ported | Missing capability: floating picture in picture window. |
| `pinDms` | `pin-dms` | partial | Pin DM submenu plus Unpin and Move pin up and down (custom order only) on every DM, categories on a settings page (no colour, no rename, 8 max), collapse settings only set the starting state, the unpinned DM list cannot be collapsed. |
| `plainFolderIcon` | `plain-folder-icon` | ported | No known limitations. |
| `platformIndicators` | `platform-indicators` | partial | List, badges and messages only decide together whether badges are emitted, client reports desktop, mobile and web only, first plugin in priority order wins each field, badges merge. |
| `previewMessage` | `preview-message` | partial | Card above the composer from a draft snapshot (not a timeline message), attachments not previewed because the composer does not expose them, 8000 byte markdown limit. |
| `quickMention` | `quick-mention` | ported | No check for the permission to send messages in the channel. |
| `quickReply` | `quick-reply` | partial | Last 100 loaded messages, no DMs without context-private, no cancel past the newest message (escape is the host), only blocked users skipped, shouldMention plugin mode pings and lets no-reply-mention decide at send. |
| `reactErrorDecoder` | | not ported | Impossible: React internals. |
| `readAllNotificationsButton` | `read-all-notifications` | partial | Mark-guild-read is limited to 5 per click so more unread servers need another click, DMs not touched. |
| `relationshipNotifier` | `relationship-notifier` | partial | Removals you cause yourself (unfriend, leave, close group) notify too since the client gives no origin; notifications grouped because of the 1 per 5 s limit; own wording, fixed title. |
| `replaceGoogleSearch` | `replace-google-search` | partial | The client has no Google entry to replace so it adds its own selection menu entry, engine list is Vencord's plus Ecosia, selection cut at 500 characters, one search per 10 seconds. |
| `replyTimestamp` | `reply-timestamp` | partial | Time comes from the id of the replied message in a utcOffset setting (no system time zone), 12 hour text with a date on other days, nothing shows when the client draws the quote as deleted. |
| `revealAllSpoilers` | `reveal-all-spoilers` | partial | Ctrl only (no Command key), channel wide reveal covers the newest 40 spoiler messages of the last 100, the click is taken by the plugin. |
| `reverseImageSearch` | `reverse-image-search` | partial | Five engines (Google Lens, Yandex, TinEye, SauceNAO, Bing), no All entry because open-url allows one per 10 seconds, IQDB and ImgOps dropped. |
| `reviewDB` | `review-db` | partial | Read only: no login, posting, votes, reports, blocked users, new review notifications or server reviews; API host is manti.vendicated.dev because reviewdb.mantikafasi.dev only serves the website. |
| `roleColorEverywhere` | `role-color-everywhere` | partial | memberList, voiceUsers, reactorsList and pollResults cannot be separated (names are colored while any is on), colorChatMessages tints the message stripe with messageSaturation instead of the text, colored mentions are plain text and lose the profile click. |
| `secretRingTone` | | not ported | Impossible: Discord's call ringtone. |
| `sendTimestamps` | `send-timestamps` | partial | Plugins cannot read the system time zone so a utcOffset setting (default +00:00) is used for the picker and the backtick shortcut, preview draws s and S like f. |
| `serverInfo` | `server-info` | partial | Friends, Blocked and Ignored tabs dropped, no join date, locale, verification level or role count, online count only when the client has it. |
| `serverListIndicators` | `server-list-indicators` | ported | Mode values are friends, servers, both; servers you only asked to join are not counted. |
| `shikiCodeblocks.desktop` | `code-highlight` | partial | Built in tokenizer for 16 languages and 4 palettes instead of Shiki, bgOpacity accepted but ignored, no custom theme, icons or Discord highlighter choice, blocks over 16 KB left plain. |
| `showAllMessageButtons` | `show-all-message-buttons` | ported | No known limitations. |
| `showConnections` | `show-connections` | partial | Popout section only with a Load button while the profile is not cached, Lucide icon map (no brand logos), the client gives no connection url yet so names are copied instead of linked. |
| `showHiddenChannels` | `see-hidden-channels` | ported | Hidden channels stay in the list, dimmed with a lock, and cannot be opened. |
| `showHiddenThings` | | not ported | Policy: bypasses client side permission checks. |
| `showMeYourName` | `show-me-your-name` | partial | inReplies dropped (the override applies to messages, replies, member list and popout together), no streamer mode shortening, first plugin in priority order wins each field, badges merge. |
| `showTimeoutDuration` | `show-timeout-duration` | partial | Own clock badge (not the timeout icon), whole minutes refreshed every 60 s while a timed out member is shown, displayStyle is tooltip or inline. |
| `silentMessageToggle` | `silent-message-toggle` | ported | No known limitations. |
| `silentTyping` | `silent-type` | ported | No known limitations. |
| `sortFriendRequests` | `sort-friend-requests` | partial | Separate Requests by date tab, the core pending list keeps its order, dates drawn as short dates. |
| `spotifyControls` | | not ported | Missing capability: Spotify playback control and an account panel player. |
| `spotifyCrack` | | not ported | Policy: spoofs Spotify premium. |
| `spotifyShareCommands` | | not ported | Missing capability: reading your own Spotify activity. |
| `startupTimings` | `startup-timings` | partial | Lists the client startup phases only (no per plugin load times), refreshes once after 5 s and with a button. |
| `stickerPaste` | `sticker-paste` | partial | Own toolbar picker (max 60, guild plus packs, no Lottie) attaches to the draft, the native sticker picker still sends on pick. |
| `streamerModeOnStream` | | not ported | Missing capability: own stream start and stop events. |
| `superReactionTweaks` | `super-reaction-tweaks` | partial | unlimitedSuperReactionPlaying and superReactionPlayingLimit accepted but not applicable, adds a Super react reaction menu entry. |
| `textReplace` | `text-replace` | partial | Regex-lite engine (no look-around or backreferences, flags u y v ignored), only $1 to $9 groups, no drag to reorder, no exception for the rules sharing channel. |
| `themeAttributes` | | not ported | Impossible: DOM attributes. |
| `translate` | `translate` | partial | Google Translate and DeepL Free only (no DeepL Pro, Kagi or auto-translate on send), languages are settings not a picker, translation drawn as an accessory block (annotations cap at 200 bytes), no DMs. |
| `typingIndicator` | `typing-indicator` | partial | Badge icon or icon with count instead of dots or avatars, includeIgnoredUsers dropped, threads not decorated. |
| `typingTweaks` | | not ported | Missing capability: replacing the typing line of the composer. |
| `unindent` | `unindent` | ported | No known limitations. |
| `unlockedAvatarZoom` | | not ported | Impossible: Discord's avatar viewer. |
| `unsuppressEmbeds` | | not ported | Missing capability: message flag update endpoint, plugins have no raw REST. |
| `userMessagesPronouns` | `user-pronouns` | partial | Global profile pronouns only (no per server pronouns), profiles load one every 2 s from a timer for the authors the display hook saw so pronouns fill in over time, bots skipped, one display plugin wins the pronouns field. |
| `userVoiceShow` | `user-voice-show` | partial | Only users in the call you are in get the badge (no other voice states are exposed), the three show settings only decide together whether badges are emitted, first plugin in priority order wins each field, badges merge. |
| `usrbg` | `usrbg` | partial | voiceBackground dropped, nitroFirst only sees profiles the client already loaded, the 6 MB index is fetched within the 8 MB response limit, stored as a 2.5 MB sorted table refreshed daily, stops working once the index passes the 8 MB fetch cap. |
| `validReply` | `valid-reply` | partial | The reply quote reads message.referenced_message and not a cache so it cannot be repaired and reply-preview mounts only draw for resolved quotes, the fetched message shows as a line with a Jump button under the reply instead, fetches run for every unresolved reply and not on hover. |
| `validUser` | `valid-user` | ported | Loads for every unknown mention seen, not on hover, the mention keeps the client node and shows the name because the loaded user is now known to the client. |
| `vcDoubleClick` | `vc-double-click` | partial | The host consumes every plain channel click while the plugin is on and the plugin reopens non-voice channels itself, modified and middle clicks pass through, a single click on a voice channel only opens it when already connected. |
| `vcNarrator` | `vc-narrator` | partial | Speech is Windows only today, one line per 2 s so simultaneous lines are joined and cut at 300 chars and lines that hit the limit are queued and flushed on a timer, DM calls are not narrated without context-private, mute and deafen are own state only, other users moving show as leave, display names instead of usernames, no voice choice, rate limited to 0.5 to 2, a 1 s check runs during a call to notice leaving because it sends no event. |
| `vencordToolbox` | `toolbox` | partial | Popover with plugin settings, copy version and the enabled plugin list (100 max, read at start and on Refresh), no enable or disable. |
| `viewIcons` | `view-icons` | partial | Context menu entries only (member avatar and banner, server icon and banner), no avatar click in the popout, server avatar, avatar decoration or group chat icon. |
| `viewRaw` | `view-raw` | partial | JSON built from the client model not the Discord payload, messages, members and channels only (no servers, roles, profiles or DMs), stripSensitiveData dropped, clickMethod is button or menu (no right click copy), lines over 3.5 KB cut in the window. |
| `voiceDownload` | `voice-download` | ported | No settings, button shows on audio attachments only and needs message data so it is absent in DMs, uses the native save dialog instead of a browser tab. |
| `voiceMessages` | | not ported | Missing capability: microphone capture for plugins. |
| `volumeBooster` | | not ported | Impossible: WebRTC audio graph. |
| `webContextMenus.web` | | not ported | Impossible: the client draws link, image and selection menus natively. |
| `webKeybinds.web` | `web-keybinds` | partial | overrideCommonKeybinds dropped, ctrl+comma and cmd+comma not bound (the client opens settings with them natively and plugins cannot bind them). |
| `webPWA.browser` | | not ported | Impossible: browser installation feature. |
| `webScreenShare.browser` | | not ported | Impossible: browser screen sharing. |
| `webScreenShareFixes.web` | | not ported | Impossible: browser screen sharing. |
| `whoReacted` | `who-reacted` | partial | Loads on click of a toolbar button or the reaction menu (host allows 5 loads then one per 2 s), 10 users per emoji, clickableAvatars switches user chips for plain avatars. |
| `xsOverlay` | | not ported | Missing capability: local socket to XSOverlay, plugins have no sockets. |
| `youtubeAdblock.desktop` | | not ported | Impossible: request interception in the app process. |
