use std::path::PathBuf;

pub fn pick_files(multiple: bool) -> Vec<PathBuf> {
	let dialog = rfd::FileDialog::new();
	if multiple {
		dialog.pick_files().unwrap_or_default()
	} else {
		dialog.pick_file().into_iter().collect()
	}
}

pub fn save_file(file_name: &str) -> Option<PathBuf> {
	rfd::FileDialog::new().set_file_name(file_name).save_file()
}
