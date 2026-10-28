use crate::scan::Kind;

pub struct Palette {
	keyword: u32,
	type_name: u32,
	function: u32,
	string: u32,
	number: u32,
	comment: u32,
	punctuation: u32,
	property: u32,
	variable: u32,
}

static DARK_PLUS: Palette = Palette {
	keyword: 0x569cd6,
	type_name: 0x4ec9b0,
	function: 0xdcdcaa,
	string: 0xce9178,
	number: 0xb5cea8,
	comment: 0x6a9955,
	punctuation: 0x9a9a9a,
	property: 0x9cdcfe,
	variable: 0x9cdcfe,
};

static ONE_DARK: Palette = Palette {
	keyword: 0xc678dd,
	type_name: 0xe5c07b,
	function: 0x61afef,
	string: 0x98c379,
	number: 0xd19a66,
	comment: 0x5c6370,
	punctuation: 0x8a919d,
	property: 0xe06c75,
	variable: 0xe06c75,
};

static MONOKAI: Palette = Palette {
	keyword: 0xf92672,
	type_name: 0x66d9ef,
	function: 0xa6e22e,
	string: 0xe6db74,
	number: 0xae81ff,
	comment: 0x75715e,
	punctuation: 0xb0b0a8,
	property: 0xfd971f,
	variable: 0xfd971f,
};

static GITHUB_LIGHT: Palette = Palette {
	keyword: 0xd73a49,
	type_name: 0x6f42c1,
	function: 0x6f42c1,
	string: 0x032f62,
	number: 0x005cc5,
	comment: 0x6a737d,
	punctuation: 0x586069,
	property: 0x005cc5,
	variable: 0xe36209,
};

impl Palette {
	pub fn find(name: &str) -> &'static Palette {
		match name.trim() {
			"one-dark" => &ONE_DARK,
			"monokai" => &MONOKAI,
			"github-light" => &GITHUB_LIGHT,
			_ => &DARK_PLUS,
		}
	}

	pub fn color(&self, kind: Kind) -> Option<u32> {
		match kind {
			Kind::Plain => None,
			Kind::Keyword => Some(self.keyword),
			Kind::Type => Some(self.type_name),
			Kind::Function => Some(self.function),
			Kind::String => Some(self.string),
			Kind::Number => Some(self.number),
			Kind::Comment => Some(self.comment),
			Kind::Punctuation => Some(self.punctuation),
			Kind::Property => Some(self.property),
			Kind::Variable => Some(self.variable),
		}
	}
}
