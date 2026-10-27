use plugin_sdk::{Plugin, export_plugin};

#[derive(Default)]
struct SeeHiddenChannels;

impl Plugin for SeeHiddenChannels {}

export_plugin!(SeeHiddenChannels);
