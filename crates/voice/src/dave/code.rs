const GROUP_DIGITS: usize = 5;
const PRIVACY_DIGITS: usize = 30;
const PAIRWISE_DIGITS: usize = 45;

pub(super) fn privacy_code(authenticator: &[u8]) -> Option<String> {
	displayable_code(authenticator, PRIVACY_DIGITS)
}

pub(super) fn pairwise_display(fingerprint: &[u8]) -> Option<String> {
	displayable_code(fingerprint, PAIRWISE_DIGITS)
}

fn displayable_code(bytes: &[u8], digits: usize) -> Option<String> {
	if bytes.len() < digits {
		return None;
	}
	let modulus = 10u64.pow(GROUP_DIGITS as u32);
	let mut code = String::with_capacity(digits);
	for group in bytes
		.as_chunks::<GROUP_DIGITS>()
		.0
		.iter()
		.take(digits / GROUP_DIGITS)
	{
		let value = group
			.iter()
			.fold(0u64, |value, byte| (value << 8) | u64::from(*byte))
			% modulus;
		code.push_str(&format!("{value:0width$}", width = GROUP_DIGITS));
	}
	Some(code)
}
