use std::time::{Duration, Instant};

use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
	D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
	D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
	ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{
	DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, HDC,
	HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::System::WinRT::Direct3D11::{
	CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
	EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
	GetWindowTextW, IsIconic, IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, Interface, PCWSTR};

use super::super::{ScreenTarget, ScreenTargetKind, VideoSource};
use crate::video::{PixelFormat, VideoError, VideoFrame, mf};

const MONITORINFOF_PRIMARY: u32 = 1;
const FRAME_POOL_BUFFERS: i32 = 2;
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(2);
const BGRA_BYTES: usize = 4;

fn failure(error: windows::core::Error) -> VideoError {
	mf::device_error(error)
}

pub struct ScreenSource {
	device: ID3D11Device,
	context: ID3D11DeviceContext,
	winrt_device: IDirect3DDevice,
	pool: Direct3D11CaptureFramePool,
	session: GraphicsCaptureSession,
	staging: Option<(ID3D11Texture2D, u32, u32)>,
	pool_size: (u32, u32),
	last: Option<VideoFrame>,
	interval: Duration,
	next: Instant,
	started: Instant,
}

unsafe impl Send for ScreenSource {}

impl ScreenSource {
	pub fn list() -> Result<Vec<ScreenTarget>, VideoError> {
		let mut targets = monitors();
		targets.extend(windows_on_desktop());
		Ok(targets)
	}

	pub fn open(target: &ScreenTarget, fps: u32) -> Result<Self, VideoError> {
		mf::prepare()?;
		if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
			return Err(VideoError::Unsupported);
		}
		let interop =
			windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(failure)?;
		let item: GraphicsCaptureItem = unsafe {
			match target.kind {
				ScreenTargetKind::Monitor => interop.CreateForMonitor(HMONITOR(target.id as isize as *mut _)),
				ScreenTargetKind::Window => interop.CreateForWindow(HWND(target.id as isize as *mut _)),
			}
		}
		.map_err(|_| VideoError::NotFound)?;
		let (device, context) = create_device()?;
		let dxgi = device.cast::<IDXGIDevice>().map_err(failure)?;
		let winrt_device = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
			.and_then(|inspectable| inspectable.cast::<IDirect3DDevice>())
			.map_err(failure)?;
		let size = item.Size().map_err(failure)?;
		let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
			&winrt_device,
			DirectXPixelFormat::B8G8R8A8UIntNormalized,
			FRAME_POOL_BUFFERS,
			size,
		)
		.map_err(failure)?;
		let session = pool.CreateCaptureSession(&item).map_err(failure)?;
		let _ = session.SetIsCursorCaptureEnabled(true);
		session.StartCapture().map_err(failure)?;
		let now = Instant::now();
		let interval = Duration::from_secs(1) / fps.max(1);
		Ok(Self {
			device,
			context,
			winrt_device,
			pool,
			session,
			staging: None,
			pool_size: (size.Width as u32, size.Height as u32),
			last: None,
			interval,
			next: now,
			started: now,
		})
	}

	fn latest_surface_frame(&mut self) -> Result<Option<VideoFrame>, VideoError> {
		let mut latest = None;
		while let Ok(frame) = self.pool.TryGetNextFrame() {
			latest = Some(frame);
		}
		let Some(frame) = latest else {
			return Ok(None);
		};
		let content = frame.ContentSize().map_err(failure)?;
		let (width, height) = (content.Width as u32, content.Height as u32);
		let surface = frame.Surface().map_err(failure)?;
		let access = surface.cast::<IDirect3DDxgiInterfaceAccess>().map_err(failure)?;
		let texture: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(failure)?;
		let data = self.read_texture(&texture, width, height)?;
		let resized = (width, height) != self.pool_size;
		drop(surface);
		drop(frame);
		if resized {
			self.pool_size = (width, height);
			self.pool
				.Recreate(
					&self.winrt_device,
					DirectXPixelFormat::B8G8R8A8UIntNormalized,
					FRAME_POOL_BUFFERS,
					SizeInt32 {
						Width: width as i32,
						Height: height as i32,
					},
				)
				.map_err(failure)?;
		}
		Ok(Some(VideoFrame {
			width,
			height,
			format: PixelFormat::Bgra,
			data,
			timestamp: self.started.elapsed(),
		}))
	}

	fn read_texture(
		&mut self,
		texture: &ID3D11Texture2D,
		width: u32,
		height: u32,
	) -> Result<Vec<u8>, VideoError> {
		let mut description = D3D11_TEXTURE2D_DESC::default();
		unsafe { texture.GetDesc(&mut description) };
		let staging = self.staging_for(&description)?;
		unsafe {
			self.context.CopyResource(&staging, texture);
			let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
			self.context
				.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
				.map_err(failure)?;
			let row_bytes = width as usize * BGRA_BYTES;
			let mut data = Vec::with_capacity(row_bytes * height as usize);
			let rows = height.min(description.Height) as usize;
			let columns = row_bytes.min(description.Width as usize * BGRA_BYTES);
			for row in 0..rows {
				let source = std::slice::from_raw_parts(
					mapped.pData.cast::<u8>().add(row * mapped.RowPitch as usize),
					columns,
				);
				data.extend_from_slice(source);
				data.resize(data.len() + row_bytes - columns, 0);
			}
			data.resize(row_bytes * height as usize, 0);
			self.context.Unmap(&staging, 0);
			Ok(data)
		}
	}

	fn staging_for(&mut self, description: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D, VideoError> {
		if let Some((texture, width, height)) = &self.staging
			&& *width == description.Width
			&& *height == description.Height
		{
			return Ok(texture.clone());
		}
		let mut staging_description = *description;
		staging_description.Usage = D3D11_USAGE_STAGING;
		staging_description.BindFlags = 0;
		staging_description.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
		staging_description.MiscFlags = 0;
		let mut texture = None;
		unsafe {
			self.device
				.CreateTexture2D(&staging_description, None, Some(&mut texture))
		}
		.map_err(failure)?;
		let texture = texture.ok_or(VideoError::NotFound)?;
		self.staging = Some((texture.clone(), description.Width, description.Height));
		Ok(texture)
	}
}

impl Drop for ScreenSource {
	fn drop(&mut self) {
		let _ = self.session.Close();
		let _ = self.pool.Close();
	}
}

impl VideoSource for ScreenSource {
	fn next_frame(&mut self) -> Result<VideoFrame, VideoError> {
		let now = Instant::now();
		if self.next > now {
			std::thread::sleep(self.next - now);
		}
		let waiting_since = Instant::now();
		let fresh = loop {
			if let Some(frame) = self.latest_surface_frame()? {
				break Some(frame);
			}
			if self.last.is_some() {
				break None;
			}
			if waiting_since.elapsed() > FIRST_FRAME_TIMEOUT {
				return Err(VideoError::Timeout);
			}
			std::thread::sleep(POLL_INTERVAL);
		};
		let now = Instant::now();
		self.next = (self.next + self.interval).max(now);
		let frame = match fresh {
			Some(frame) => {
				self.last = Some(frame.clone());
				frame
			}
			None => {
				let mut repeated = self.last.clone().ok_or(VideoError::Timeout)?;
				repeated.timestamp = self.started.elapsed();
				repeated
			}
		};
		Ok(frame)
	}

	fn size(&self) -> (u32, u32) {
		self.last
			.as_ref()
			.map_or(self.pool_size, |frame| (frame.width, frame.height))
	}

	fn set_fps(&mut self, fps: u32) {
		self.interval = Duration::from_secs(1) / fps.max(1);
	}
}

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext), VideoError> {
	let mut last = VideoError::NotFound;
	for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
		match try_device(driver) {
			Ok(pair) => return Ok(pair),
			Err(error) => last = failure(error),
		}
	}
	Err(last)
}

fn try_device(driver: D3D_DRIVER_TYPE) -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
	let mut device = None;
	let mut context = None;
	unsafe {
		D3D11CreateDevice(
			None,
			driver,
			HMODULE::default(),
			D3D11_CREATE_DEVICE_BGRA_SUPPORT,
			None,
			D3D11_SDK_VERSION,
			Some(&mut device),
			None,
			Some(&mut context),
		)?;
	}
	match (device, context) {
		(Some(device), Some(context)) => Ok((device, context)),
		_ => Err(windows::core::Error::from_hresult(
			windows::Win32::Foundation::E_FAIL,
		)),
	}
}

fn monitors() -> Vec<ScreenTarget> {
	unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, found: LPARAM) -> BOOL {
		let targets = unsafe { &mut *(found.0 as *mut Vec<ScreenTarget>) };
		let mut info = MONITORINFOEXW::default();
		info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
		if unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) }.as_bool() {
			let (width, height) = physical_size(&info);
			let name = String::from_utf16_lossy(&info.szDevice)
				.trim_end_matches('\0')
				.to_owned();
			targets.push(ScreenTarget {
				kind: ScreenTargetKind::Monitor,
				id: monitor.0 as usize as u64,
				name,
				width,
				height,
				primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
			});
		}
		BOOL::from(true)
	}
	let mut targets: Vec<ScreenTarget> = Vec::new();
	unsafe {
		let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM((&raw mut targets) as isize));
	}
	targets
}

fn physical_size(info: &MONITORINFOEXW) -> (u32, u32) {
	let mut mode = DEVMODEW {
		dmSize: std::mem::size_of::<DEVMODEW>() as u16,
		..Default::default()
	};
	if unsafe { EnumDisplaySettingsW(PCWSTR(info.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode) }
		.as_bool()
	{
		return (mode.dmPelsWidth, mode.dmPelsHeight);
	}
	let rect = info.monitorInfo.rcMonitor;
	((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32)
}

fn windows_on_desktop() -> Vec<ScreenTarget> {
	unsafe extern "system" fn collect(window: HWND, found: LPARAM) -> BOOL {
		let targets = unsafe { &mut *(found.0 as *mut Vec<ScreenTarget>) };
		if let Some(target) = describe_window(window) {
			targets.push(target);
		}
		BOOL::from(true)
	}
	let mut targets: Vec<ScreenTarget> = Vec::new();
	unsafe {
		let _ = EnumWindows(Some(collect), LPARAM((&raw mut targets) as isize));
	}
	targets
}

fn describe_window(window: HWND) -> Option<ScreenTarget> {
	unsafe {
		if !IsWindowVisible(window).as_bool() || IsIconic(window).as_bool() {
			return None;
		}
		if GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0
			|| GetWindow(window, GW_OWNER).is_ok_and(|owner| !owner.is_invalid())
		{
			return None;
		}
		let mut cloaked = 0u32;
		let _ = DwmGetWindowAttribute(
			window,
			DWMWA_CLOAKED,
			(&raw mut cloaked).cast(),
			std::mem::size_of::<u32>() as u32,
		);
		if cloaked != 0 {
			return None;
		}
		let length = GetWindowTextLengthW(window);
		if length == 0 {
			return None;
		}
		let mut title = vec![0u16; length as usize + 1];
		let copied = GetWindowTextW(window, &mut title);
		let mut rect = RECT::default();
		GetWindowRect(window, &mut rect).ok()?;
		let (width, height) = ((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32);
		(copied > 0 && width > 0 && height > 0).then(|| ScreenTarget {
			kind: ScreenTargetKind::Window,
			id: window.0 as usize as u64,
			name: String::from_utf16_lossy(&title[..copied as usize]),
			width,
			height,
			primary: false,
		})
	}
}
