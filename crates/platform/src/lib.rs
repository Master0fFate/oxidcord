pub mod autostart;
pub mod clipboard;
pub mod hotkeys;
pub mod idle;
pub mod notifications;
pub mod open;
pub mod relaunch;
pub mod single_instance;
pub mod sound;
pub mod system_theme;
pub mod tray;

mod error;

pub use error::{PlatformError, Result};
