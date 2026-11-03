use serde::{Deserialize, Serialize};

use crate::emoji::PartialEmoji;
use crate::lenient::{LenientVec, int_enum};
use crate::message::AttachmentFlags;
use crate::snowflake::Snowflake;

const SCAN_EXPLICIT: u64 = 1 << 0;
const SCAN_GORE: u64 = 1 << 1;
const SCAN_SELF_HARM: u64 = 1 << 2;

int_enum! {
	pub enum ComponentType {
		ActionRow = 1,
		Button = 2,
		StringSelect = 3,
		TextInput = 4,
		UserSelect = 5,
		RoleSelect = 6,
		MentionableSelect = 7,
		ChannelSelect = 8,
		Section = 9,
		TextDisplay = 10,
		Thumbnail = 11,
		MediaGallery = 12,
		File = 13,
		Separator = 14,
		Container = 17,
		Label = 18,
		FileUpload = 19,
		RadioGroup = 21,
		CheckboxGroup = 22,
		Checkbox = 23,
	}
}

impl ComponentType {
	pub fn is_select(self) -> bool {
		matches!(
			self,
			Self::StringSelect
				| Self::UserSelect
				| Self::RoleSelect
				| Self::MentionableSelect
				| Self::ChannelSelect
		)
	}
}

int_enum! {
	pub enum ButtonStyle {
		Primary = 1,
		Secondary = 2,
		Success = 3,
		Danger = 4,
		Link = 5,
		Premium = 6,
	}
}

int_enum! {
	pub enum TextInputStyle {
		Short = 1,
		Paragraph = 2,
	}
}

int_enum! {
	pub enum SeparatorSpacing {
		Small = 1,
		Large = 2,
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UnfurledMedia {
	pub url: String,
	pub proxy_url: Option<String>,
	pub height: Option<i64>,
	pub width: Option<i64>,
	pub content_type: Option<String>,
	pub attachment_id: Option<Snowflake>,
	pub content_scan_metadata: Option<ContentScan>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ContentScan {
	pub flags: Option<u64>,
}

impl UnfurledMedia {
	pub fn sensitivity(&self) -> AttachmentFlags {
		let scanned = self
			.content_scan_metadata
			.as_ref()
			.and_then(|scan| scan.flags)
			.unwrap_or_default();
		[
			(SCAN_EXPLICIT, AttachmentFlags::CONTAINS_EXPLICIT_CONTENT),
			(SCAN_GORE, AttachmentFlags::CONTAINS_GORE_CONTENT),
			(SCAN_SELF_HARM, AttachmentFlags::CONTAINS_SELF_HARM_CONTENT),
		]
		.into_iter()
		.filter(|(bit, _)| scanned & bit != 0)
		.fold(AttachmentFlags::empty(), |all, (_, flag)| all | flag)
	}
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GalleryItem {
	pub media: UnfurledMedia,
	pub description: Option<String>,
	pub spoiler: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectOption {
	pub label: String,
	pub value: String,
	pub description: Option<String>,
	pub emoji: Option<PartialEmoji>,
	pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Component {
	#[serde(rename = "type")]
	pub kind: ComponentType,
	pub id: Option<i64>,
	pub custom_id: Option<String>,
	pub components: LenientVec<Component>,
	pub accessory: Option<Box<Component>>,
	pub content: Option<String>,
	pub style: Option<i64>,
	pub label: Option<String>,
	pub emoji: Option<PartialEmoji>,
	pub url: Option<String>,
	pub sku_id: Option<Snowflake>,
	pub disabled: bool,
	pub placeholder: Option<String>,
	pub options: LenientVec<SelectOption>,
	pub min_values: Option<u32>,
	pub max_values: Option<u32>,
	pub media: Option<UnfurledMedia>,
	pub description: Option<String>,
	pub spoiler: bool,
	pub items: LenientVec<GalleryItem>,
	pub file: Option<UnfurledMedia>,
	pub name: Option<String>,
	pub size: Option<i64>,
	pub divider: Option<bool>,
	pub spacing: Option<SeparatorSpacing>,
	pub accent_color: Option<u32>,
	pub component: Option<Box<Component>>,
	pub value: Option<String>,
	pub min_length: Option<u32>,
	pub max_length: Option<u32>,
	pub required: Option<bool>,
	pub default: bool,
}

impl Component {
	pub fn button_style(&self) -> ButtonStyle {
		self.style.map_or(ButtonStyle::Secondary, ButtonStyle::from)
	}

	pub fn text_input_style(&self) -> TextInputStyle {
		self.style.map_or(TextInputStyle::Short, TextInputStyle::from)
	}

	pub fn is_required(&self) -> bool {
		self.required.unwrap_or(true)
	}
}
