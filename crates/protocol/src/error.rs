use std::fmt;

use serde_json::error::Category;
use thiserror::Error;

#[derive(Error)]
#[error("failed to decode {event}: {} error at line {} column {}", category_name(.error), .error.line(), .error.column())]
pub struct DecodeError {
	event: String,
	error: serde_json::Error,
}

impl DecodeError {
	pub fn new(event: impl Into<String>, error: serde_json::Error) -> Self {
		Self {
			event: event.into(),
			error,
		}
	}

	pub fn event(&self) -> &str {
		&self.event
	}

	pub fn category(&self) -> Category {
		self.error.classify()
	}
}

fn category_name(error: &serde_json::Error) -> &'static str {
	match error.classify() {
		Category::Io => "io",
		Category::Syntax => "syntax",
		Category::Data => "data",
		Category::Eof => "eof",
	}
}

impl fmt::Debug for DecodeError {
	fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		formatter
			.debug_struct("DecodeError")
			.field("event", &self.event)
			.field("category", &category_name(&self.error))
			.field("line", &self.error.line())
			.field("column", &self.error.column())
			.finish()
	}
}
