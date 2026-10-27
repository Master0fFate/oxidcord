use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::Authorization::{SE_FILE_OBJECT, SetSecurityInfo};
use windows::Win32::Security::{
	ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, DACL_SECURITY_INFORMATION, GetLengthSid,
	GetTokenInformation, InitializeAcl, PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
	TokenUser,
};
use windows::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub(crate) fn restrict_to_current_user(file: &File) -> io::Result<()> {
	let user = current_user()?;
	let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
	let acl_len = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() + unsafe { GetLengthSid(sid) } as usize;
	let mut acl = vec![0u64; acl_len.div_ceil(size_of::<u64>())];
	let acl = acl.as_mut_ptr().cast::<ACL>();
	unsafe {
		InitializeAcl(acl, acl_len as u32, ACL_REVISION)?;
		AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS.0, sid)?;
	}
	let status = unsafe {
		SetSecurityInfo(
			HANDLE(file.as_raw_handle()),
			SE_FILE_OBJECT,
			DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
			None,
			None,
			Some(acl),
			None,
		)
	};
	status.ok().map_err(io::Error::from)
}

fn current_user() -> io::Result<Vec<u64>> {
	let mut token = HANDLE::default();
	unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)? };
	let mut needed = 0;
	let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
	let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
	let read = unsafe {
		GetTokenInformation(
			token,
			TokenUser,
			Some(buffer.as_mut_ptr().cast()),
			needed,
			&mut needed,
		)
	};
	let _ = unsafe { CloseHandle(token) };
	read?;
	Ok(buffer)
}
