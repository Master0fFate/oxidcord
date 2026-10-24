use crate::bindings::oxidcord::plugin::client;
use crate::types::HostError;

pub fn once(delay_ms: u32, tag: &str) -> Result<u64, HostError> {
	client::set_timer(delay_ms, 0, tag)
}

pub fn every(interval_ms: u32, tag: &str) -> Result<u64, HostError> {
	client::set_timer(interval_ms, interval_ms, tag)
}

pub fn set(delay_ms: u32, repeat_ms: u32, tag: &str) -> Result<u64, HostError> {
	client::set_timer(delay_ms, repeat_ms, tag)
}

pub fn clear(id: u64) {
	client::clear_timer(id);
}
