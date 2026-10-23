use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
	println!("cargo:rerun-if-env-changed=OXIDCORD_GIT_SHA");
	println!("cargo:rerun-if-env-changed=OXIDCORD_BUILD_DATE");
	println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
	println!("cargo:rerun-if-changed=../../.git/HEAD");
	if let Some(sha) = std::env::var("OXIDCORD_GIT_SHA").ok().or_else(git_sha) {
		println!("cargo:rustc-env=OXIDCORD_GIT_SHA={sha}");
	}
	let date = std::env::var("OXIDCORD_BUILD_DATE").unwrap_or_else(|_| build_date());
	println!("cargo:rustc-env=OXIDCORD_BUILD_DATE={date}");
}

fn git_sha() -> Option<String> {
	let output = Command::new("git")
		.args(["rev-parse", "--short=12", "HEAD"])
		.output()
		.ok()?;
	let sha = String::from_utf8(output.stdout).ok()?.trim().to_owned();
	(output.status.success() && !sha.is_empty()).then_some(sha)
}

fn build_date() -> String {
	let seconds = std::env::var("SOURCE_DATE_EPOCH")
		.ok()
		.and_then(|value| value.parse::<i64>().ok())
		.unwrap_or_else(|| {
			SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.map_or(0, |elapsed| elapsed.as_secs() as i64)
		});
	let days = seconds.div_euclid(86_400) + 719_468;
	let era = days.div_euclid(146_097);
	let day_of_era = days.rem_euclid(146_097);
	let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
	let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
	let month_index = (5 * day_of_year + 2) / 153;
	let day = day_of_year - (153 * month_index + 2) / 5 + 1;
	let month = if month_index < 10 {
		month_index + 3
	} else {
		month_index - 9
	};
	let year = year_of_era + era * 400 + i64::from(month <= 2);
	format!("{year:04}-{month:02}-{day:02}")
}
