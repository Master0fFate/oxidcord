#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Process {
	pub pid: u32,
	pub path: String,
}

fn normalized(path: &str) -> String {
	path.replace('\\', "/").to_lowercase()
}

#[cfg(windows)]
pub(crate) fn running() -> Vec<Process> {
	use windows::Win32::Foundation::CloseHandle;
	use windows::Win32::System::Diagnostics::ToolHelp::{
		CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
	};
	use windows::Win32::System::Threading::{
		OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
	};
	use windows::core::PWSTR;

	const PATH_CAPACITY: usize = 1024;

	let mut processes = Vec::new();
	let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
		return processes;
	};
	let mut entry = PROCESSENTRY32W {
		dwSize: size_of::<PROCESSENTRY32W>() as u32,
		..Default::default()
	};
	let mut buffer = [0u16; PATH_CAPACITY];
	let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
	while more {
		let pid = entry.th32ProcessID;
		let mut path = None;
		if let Ok(handle) = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
			let mut length = PATH_CAPACITY as u32;
			if unsafe {
				QueryFullProcessImageNameW(
					handle,
					PROCESS_NAME_WIN32,
					PWSTR(buffer.as_mut_ptr()),
					&mut length,
				)
			}
			.is_ok()
			{
				path = Some(String::from_utf16_lossy(&buffer[..length as usize]));
			}
			let _ = unsafe { CloseHandle(handle) };
		}
		let path = path.unwrap_or_else(|| {
			let end = entry
				.szExeFile
				.iter()
				.position(|&c| c == 0)
				.unwrap_or(entry.szExeFile.len());
			String::from_utf16_lossy(&entry.szExeFile[..end])
		});
		if !path.is_empty() {
			processes.push(Process {
				pid,
				path: normalized(&path),
			});
		}
		more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
	}
	let _ = unsafe { CloseHandle(snapshot) };
	processes
}

#[cfg(target_os = "linux")]
pub(crate) fn running() -> Vec<Process> {
	let Ok(entries) = std::fs::read_dir("/proc") else {
		return Vec::new();
	};
	entries
		.flatten()
		.filter_map(|entry| {
			let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
			let path = std::fs::read_link(entry.path().join("exe"))
				.ok()
				.map(|path| path.to_string_lossy().into_owned())
				.or_else(|| {
					std::fs::read_to_string(entry.path().join("comm"))
						.ok()
						.map(|comm| comm.trim().to_owned())
				})
				.filter(|path| !path.is_empty())?;
			Some(Process {
				pid,
				path: normalized(&path),
			})
		})
		.collect()
}

#[cfg(target_os = "macos")]
pub(crate) fn running() -> Vec<Process> {
	use std::ffi::{c_int, c_void};

	#[link(name = "proc")]
	unsafe extern "C" {
		fn proc_listallpids(buffer: *mut c_void, buffersize: c_int) -> c_int;
		fn proc_pidpath(pid: c_int, buffer: *mut c_void, buffersize: u32) -> c_int;
	}

	const PATH_CAPACITY: usize = 4096;

	const PID_BYTES: usize = size_of::<c_int>();
	const PID_SLACK: usize = 64;

	let bytes = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
	if bytes <= 0 {
		return Vec::new();
	}
	let mut pids = vec![0 as c_int; bytes as usize / PID_BYTES + PID_SLACK];
	let filled = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * PID_BYTES) as c_int) };
	if filled <= 0 {
		return Vec::new();
	}
	pids.truncate(filled as usize / PID_BYTES);
	let mut buffer = vec![0u8; PATH_CAPACITY];
	pids.into_iter()
		.filter(|&pid| pid > 0)
		.filter_map(|pid| {
			let written = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), PATH_CAPACITY as u32) };
			if written <= 0 {
				return None;
			}
			let path = String::from_utf8_lossy(&buffer[..written as usize]).into_owned();
			Some(Process {
				pid: pid as u32,
				path: normalized(&path),
			})
		})
		.collect()
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(crate) fn running() -> Vec<Process> {
	Vec::new()
}
