pub mod actions;
pub mod app;
mod assets;
pub mod components;
pub mod data;
mod desktop;
pub mod emoji;
pub mod format;
pub mod i18n;
pub mod icons;
pub mod images;
pub mod lookup;
pub mod model;
pub mod playback;
pub mod shell;
pub mod theme;
pub mod webframe;
pub mod zoom;

pub use app::{AppFlags, LaunchConfig, StartSession, run};

pub const APP_ID: &str = "dev.oxidcord.Oxidcord";
