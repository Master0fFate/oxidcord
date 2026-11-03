use protocol::{AvatarDecorationData, DisplayNameStyles, Nameplate, PrimaryGuild, Snowflake};

use crate::state::State;

#[derive(Debug, Clone, Copy, Default)]
pub struct Cosmetics<'a> {
	pub decoration: Option<&'a AvatarDecorationData>,
	pub tag: Option<&'a PrimaryGuild>,
	pub nameplate: Option<&'a Nameplate>,
	pub name_style: Option<&'a DisplayNameStyles>,
}

impl State {
	pub fn cosmetics(&self, guild_id: Option<Snowflake>, user_id: Snowflake) -> Cosmetics<'_> {
		let user = self.users.get(&user_id);
		let member = guild_id
			.and_then(|id| self.guilds.get(&id))
			.and_then(|guild| guild.members.get(&user_id));
		let has_asset =
			|data: &&AvatarDecorationData| data.asset.as_deref().is_some_and(|asset| !asset.is_empty());
		let decoration = member
			.and_then(|member| member.avatar_decoration_data.as_ref())
			.filter(has_asset)
			.or_else(|| {
				user.and_then(|user| user.avatar_decoration_data.as_ref())
					.filter(has_asset)
			});
		let tag = user
			.and_then(|user| user.primary_guild.as_ref().or(user.clan.as_ref()))
			.filter(|tag| {
				tag.identity_enabled == Some(true)
					&& tag.identity_guild_id.is_some()
					&& tag.tag.as_deref().is_some_and(|text| !text.is_empty())
			});
		let has_plate = |plate: &&Nameplate| !plate.asset.is_empty();
		let nameplate = member
			.and_then(|member| member.collectibles.as_ref())
			.and_then(|collectibles| collectibles.nameplate.as_ref())
			.filter(has_plate)
			.or_else(|| {
				user.and_then(|user| user.collectibles.as_ref())
					.and_then(|collectibles| collectibles.nameplate.as_ref())
					.filter(has_plate)
			});
		let has_colors = |style: &&DisplayNameStyles| !style.colors.is_empty();
		let name_style = member
			.and_then(|member| member.display_name_styles.as_ref())
			.filter(has_colors)
			.or_else(|| {
				user.and_then(|user| user.display_name_styles.as_ref())
					.filter(has_colors)
			});
		Cosmetics {
			decoration,
			tag,
			nameplate,
			name_style,
		}
	}
}
