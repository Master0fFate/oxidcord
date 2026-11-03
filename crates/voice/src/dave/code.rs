const CODE_DIGITS: usize = 30;
const GROUP_DIGITS: usize = 5;

pub(super) fn privacy_code(authenticator: &[u8]) -> Option<String> {
	if authenticator.len() < CODE_DIGITS {
		return None;
	}
	let modulus = 10u64.pow(GROUP_DIGITS as u32);
	let mut code = String::with_capacity(CODE_DIGITS);
	for group in authenticator
		.as_chunks::<GROUP_DIGITS>()
		.0
		.iter()
		.take(CODE_DIGITS / GROUP_DIGITS)
	{
		let value = group
			.iter()
			.fold(0u64, |value, byte| (value << 8) | u64::from(*byte))
			% modulus;
		code.push_str(&format!("{value:0width$}", width = GROUP_DIGITS));
	}
	Some(code)
}
