use plugin_sdk::{Plugin, export_plugin};

#[derive(Default)]
struct SilentType;

impl Plugin for SilentType {}

export_plugin!(SilentType);
