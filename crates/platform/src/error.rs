pub type Result<T> = std::result::Result<T, PlatformError>;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
	#[error(transparent)]
	Io(#[from] std::io::Error),
	#[error(transparent)]
	Notification(#[from] notify_rust::error::Error),
	#[error(transparent)]
	Tray(#[from] tray_icon::Error),
	#[error(transparent)]
	TrayIcon(#[from] tray_icon::BadIcon),
	#[error(transparent)]
	Autostart(#[from] auto_launch::Error),
	#[error(transparent)]
	Clipboard(#[from] arboard::Error),
	#[error(transparent)]
	InvalidUrl(#[from] url::ParseError),
	#[error("unsupported url scheme: {0}")]
	UnsupportedScheme(String),
	#[error(transparent)]
	Hotkey(#[from] global_hotkey::Error),
	#[error(transparent)]
	HotkeyBinding(#[from] global_hotkey::hotkey::HotKeyParseError),
	#[error("no hotkey registered as {0}")]
	UnknownHotkey(String),
	#[error("{0} already created in this process")]
	AlreadyCreated(&'static str),
	#[error("{0}")]
	Os(String),
}
