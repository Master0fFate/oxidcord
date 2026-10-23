use protocol::{Snowflake, User};
use state::State;

pub struct Lookup<'a> {
	pub state: &'a State,
	pub guild: Option<Snowflake>,
}

impl Lookup<'_> {
	pub fn user(&self, id: u64) -> String {
		self.state.display_name(self.guild, Snowflake(id))
	}

	pub fn role(&self, id: u64) -> Option<(String, Option<u32>)> {
		let role = self.state.guild(self.guild?)?.roles.get(&Snowflake(id))?;
		let color = u32::try_from(role.color).ok().filter(|color| *color != 0);
		Some((role.name.clone(), color))
	}

	pub fn channel(&self, id: u64) -> Option<String> {
		self.state.channel(Snowflake(id))?.name.clone()
	}
}

pub fn user_avatar(user: &User, size: u32) -> String {
	media::cdn::avatar_url(
		user.id.get(),
		user.avatar.as_deref(),
		user.discriminator.as_deref(),
		crate::images::scaled_pixels(size),
	)
}

pub fn avatar_for(state: &State, id: Snowflake, size: u32) -> String {
	match state.user(id) {
		Some(user) => user_avatar(user, size),
		None => media::cdn::default_avatar_url(id.get(), None),
	}
}
