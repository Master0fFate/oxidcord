use protocol::Permissions;

pub struct Right {
	pub flag: Permissions,
	pub name: &'static str,
	pub hint: &'static str,
}

pub struct RightGroup {
	pub key: &'static str,
	pub rights: &'static [Right],
}

pub const GROUPS: &[RightGroup] = &[
	RightGroup {
		key: "right-group-general",
		rights: &[
			Right {
				flag: Permissions::VIEW_CHANNEL,
				name: "right-view-channels",
				hint: "right-view-channels-hint",
			},
			Right {
				flag: Permissions::MANAGE_CHANNELS,
				name: "right-manage-channels",
				hint: "right-manage-channels-hint",
			},
			Right {
				flag: Permissions::MANAGE_ROLES,
				name: "right-manage-roles",
				hint: "right-manage-roles-hint",
			},
			Right {
				flag: Permissions::CREATE_GUILD_EXPRESSIONS,
				name: "right-create-expressions",
				hint: "right-create-expressions-hint",
			},
			Right {
				flag: Permissions::MANAGE_GUILD_EXPRESSIONS,
				name: "right-manage-expressions",
				hint: "right-manage-expressions-hint",
			},
			Right {
				flag: Permissions::VIEW_AUDIT_LOG,
				name: "right-view-audit-log",
				hint: "right-view-audit-log-hint",
			},
			Right {
				flag: Permissions::MANAGE_WEBHOOKS,
				name: "right-manage-webhooks",
				hint: "right-manage-webhooks-hint",
			},
			Right {
				flag: Permissions::MANAGE_GUILD,
				name: "right-manage-server",
				hint: "right-manage-server-hint",
			},
		],
	},
	RightGroup {
		key: "right-group-membership",
		rights: &[
			Right {
				flag: Permissions::CREATE_INSTANT_INVITE,
				name: "right-create-invite",
				hint: "right-create-invite-hint",
			},
			Right {
				flag: Permissions::CHANGE_NICKNAME,
				name: "right-change-nickname",
				hint: "right-change-nickname-hint",
			},
			Right {
				flag: Permissions::MANAGE_NICKNAMES,
				name: "right-manage-nicknames",
				hint: "right-manage-nicknames-hint",
			},
			Right {
				flag: Permissions::KICK_MEMBERS,
				name: "right-kick-members",
				hint: "right-kick-members-hint",
			},
			Right {
				flag: Permissions::BAN_MEMBERS,
				name: "right-ban-members",
				hint: "right-ban-members-hint",
			},
			Right {
				flag: Permissions::MODERATE_MEMBERS,
				name: "right-timeout-members",
				hint: "right-timeout-members-hint",
			},
		],
	},
	RightGroup {
		key: "right-group-text",
		rights: &[
			Right {
				flag: Permissions::SEND_MESSAGES,
				name: "right-send-messages",
				hint: "right-send-messages-hint",
			},
			Right {
				flag: Permissions::SEND_MESSAGES_IN_THREADS,
				name: "right-send-in-threads",
				hint: "right-send-in-threads-hint",
			},
			Right {
				flag: Permissions::CREATE_PUBLIC_THREADS,
				name: "right-create-public-threads",
				hint: "right-create-public-threads-hint",
			},
			Right {
				flag: Permissions::CREATE_PRIVATE_THREADS,
				name: "right-create-private-threads",
				hint: "right-create-private-threads-hint",
			},
			Right {
				flag: Permissions::EMBED_LINKS,
				name: "right-embed-links",
				hint: "right-embed-links-hint",
			},
			Right {
				flag: Permissions::ATTACH_FILES,
				name: "right-attach-files",
				hint: "right-attach-files-hint",
			},
			Right {
				flag: Permissions::ADD_REACTIONS,
				name: "right-add-reactions",
				hint: "right-add-reactions-hint",
			},
			Right {
				flag: Permissions::USE_EXTERNAL_EMOJIS,
				name: "right-use-external-emoji",
				hint: "right-use-external-emoji-hint",
			},
			Right {
				flag: Permissions::USE_EXTERNAL_STICKERS,
				name: "right-use-external-stickers",
				hint: "right-use-external-stickers-hint",
			},
			Right {
				flag: Permissions::MENTION_EVERYONE,
				name: "right-mention-everyone",
				hint: "right-mention-everyone-hint",
			},
			Right {
				flag: Permissions::MANAGE_MESSAGES,
				name: "right-manage-messages",
				hint: "right-manage-messages-hint",
			},
			Right {
				flag: Permissions::PIN_MESSAGES,
				name: "right-pin-messages",
				hint: "right-pin-messages-hint",
			},
			Right {
				flag: Permissions::BYPASS_SLOWMODE,
				name: "right-bypass-slowmode",
				hint: "right-bypass-slowmode-hint",
			},
			Right {
				flag: Permissions::MANAGE_THREADS,
				name: "right-manage-threads",
				hint: "right-manage-threads-hint",
			},
			Right {
				flag: Permissions::READ_MESSAGE_HISTORY,
				name: "right-read-history",
				hint: "right-read-history-hint",
			},
			Right {
				flag: Permissions::SEND_TTS_MESSAGES,
				name: "right-send-tts",
				hint: "right-send-tts-hint",
			},
			Right {
				flag: Permissions::SEND_VOICE_MESSAGES,
				name: "right-send-voice-messages",
				hint: "right-send-voice-messages-hint",
			},
			Right {
				flag: Permissions::SEND_POLLS,
				name: "right-create-polls",
				hint: "right-create-polls-hint",
			},
		],
	},
	RightGroup {
		key: "right-group-voice",
		rights: &[
			Right {
				flag: Permissions::CONNECT,
				name: "right-connect",
				hint: "right-connect-hint",
			},
			Right {
				flag: Permissions::SPEAK,
				name: "right-speak",
				hint: "right-speak-hint",
			},
			Right {
				flag: Permissions::STREAM,
				name: "right-video",
				hint: "right-video-hint",
			},
			Right {
				flag: Permissions::USE_SOUNDBOARD,
				name: "right-use-soundboard",
				hint: "right-use-soundboard-hint",
			},
			Right {
				flag: Permissions::USE_EXTERNAL_SOUNDS,
				name: "right-use-external-sounds",
				hint: "right-use-external-sounds-hint",
			},
			Right {
				flag: Permissions::USE_VAD,
				name: "right-voice-activity",
				hint: "right-voice-activity-hint",
			},
			Right {
				flag: Permissions::PRIORITY_SPEAKER,
				name: "right-priority-speaker",
				hint: "right-priority-speaker-hint",
			},
			Right {
				flag: Permissions::MUTE_MEMBERS,
				name: "right-mute-members",
				hint: "right-mute-members-hint",
			},
			Right {
				flag: Permissions::DEAFEN_MEMBERS,
				name: "right-deafen-members",
				hint: "right-deafen-members-hint",
			},
			Right {
				flag: Permissions::MOVE_MEMBERS,
				name: "right-move-members",
				hint: "right-move-members-hint",
			},
		],
	},
	RightGroup {
		key: "right-group-apps",
		rights: &[
			Right {
				flag: Permissions::USE_APPLICATION_COMMANDS,
				name: "right-use-commands",
				hint: "right-use-commands-hint",
			},
			Right {
				flag: Permissions::USE_EMBEDDED_ACTIVITIES,
				name: "right-use-activities",
				hint: "right-use-activities-hint",
			},
			Right {
				flag: Permissions::USE_EXTERNAL_APPS,
				name: "right-use-external-apps",
				hint: "right-use-external-apps-hint",
			},
		],
	},
	RightGroup {
		key: "right-group-stage",
		rights: &[Right {
			flag: Permissions::REQUEST_TO_SPEAK,
			name: "right-request-to-speak",
			hint: "right-request-to-speak-hint",
		}],
	},
	RightGroup {
		key: "right-group-events",
		rights: &[
			Right {
				flag: Permissions::CREATE_EVENTS,
				name: "right-create-events",
				hint: "right-create-events-hint",
			},
			Right {
				flag: Permissions::MANAGE_EVENTS,
				name: "right-manage-events",
				hint: "right-manage-events-hint",
			},
		],
	},
	RightGroup {
		key: "right-group-advanced",
		rights: &[Right {
			flag: Permissions::ADMINISTRATOR,
			name: "right-administrator",
			hint: "right-administrator-hint",
		}],
	},
];
