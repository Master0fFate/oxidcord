use crate::host;
use crate::types::HostError;

pub fn get(key: &str) -> Result<Option<Vec<u8>>, HostError> {
	host::storage_get(key)
}

pub fn set(key: &str, value: &[u8]) -> Result<(), HostError> {
	host::storage_set(key, value)
}

pub fn delete(key: &str) -> Result<(), HostError> {
	host::storage_delete(key)
}

pub fn keys() -> Result<Vec<String>, HostError> {
	host::storage_list()
}
