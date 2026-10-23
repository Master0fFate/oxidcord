mod ast;
mod block;
mod document;
mod entity;
mod fence;
pub mod highlight;
mod inline;
mod link;

pub use ast::{GuildNavKind, Node, TimestampStyle};
pub use document::{Document, Mentions};

pub fn parse(input: &str) -> Document {
	Document {
		nodes: block::parse(input),
	}
}

pub fn parse_inline(input: &str) -> Vec<Node> {
	inline::parse(input)
}
