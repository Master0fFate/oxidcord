pub const TRACE: u8 = 0;
pub const DEBUG: u8 = 1;
pub const INFO: u8 = 2;
pub const WARN: u8 = 3;
pub const ERROR: u8 = 4;

pub fn write(level: u8, message: &str) {
	crate::host::log(level, message);
}

#[macro_export]
macro_rules! __log_trace {
	($($arg:tt)*) => { $crate::log::write($crate::log::TRACE, &::std::format!($($arg)*)) };
}

#[macro_export]
macro_rules! __log_debug {
	($($arg:tt)*) => { $crate::log::write($crate::log::DEBUG, &::std::format!($($arg)*)) };
}

#[macro_export]
macro_rules! __log_info {
	($($arg:tt)*) => { $crate::log::write($crate::log::INFO, &::std::format!($($arg)*)) };
}

#[macro_export]
macro_rules! __log_warn {
	($($arg:tt)*) => { $crate::log::write($crate::log::WARN, &::std::format!($($arg)*)) };
}

#[macro_export]
macro_rules! __log_error {
	($($arg:tt)*) => { $crate::log::write($crate::log::ERROR, &::std::format!($($arg)*)) };
}

pub use crate::{
	__log_debug as debug, __log_error as error, __log_info as info, __log_trace as trace, __log_warn as warn,
};
