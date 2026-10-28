# Custom Idle

Sets your status to idle after the time you choose without keyboard or mouse input, or never. It checks every 30 seconds, so the change can come up to 30 seconds late.

When you come back, the plugin shows a message with a Go online button and keeps you idle until you press it, or goes online by itself when `remainInIdle` is off. If you change your status yourself in the meantime the plugin leaves it alone.

Settings:
- `idleTimeout`: minutes without input before going idle, from 0 to 60. 0 turns the plugin off. Default 10.
- `remainInIdle`: stay idle after you come back until you confirm. Default on.

Limits: this sets your status to idle, it does not change when Discord itself goes idle, so the two timers are independent and the shorter one wins. Only an online status is changed to idle, do not disturb and invisible are never touched. The input time is read from the operating system and is not available on every platform.

Permissions: `context-presence`, `actions-presence`, `context-basic`, `ui-mount`, `storage`.

Port of the Vencord plugin customIdle.
