//! The window's own pixels, read from the platform's window, never the
//! screen, so nothing of another program's is in it: on Windows
//! `PrintWindow` with its full-content flag (DirectComposition, as
//! skills/change-ahjo/try-windows.ps1 reads a window), on X11 the window's
//! image from the X server. Wayland gives a program no way to read its own
//! window back, and says so as the error. Here, not in the kit, because
//! the binary is what links the platform.

use ahjo_app_kit::capture::Picture;
use ahjo_core::Error;
use gpui::Window;

/// What `window` shows now, read from the platform's window.
pub fn window(window: &Window) -> Result<Picture, Error> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let unsupported = |what: &str| {
        Error::new(
            "capture_unsupported",
            format!("a window on {what} cannot be captured"),
        )
    };
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|e| Error::new("capture_unsupported", e.to_string()))?;
    match handle.as_raw() {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => win32::capture(h.hwnd.get()),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(h) => x11::capture(h.window.get()),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(h) => x11::capture(h.window as u32),
        RawWindowHandle::Wayland(_) => Err(unsupported("Wayland")),
        _ => Err(unsupported("this platform")),
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use ahjo_app_kit::capture::Picture;
    use ahjo_core::Error;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};

    /// The X window `id`'s image, from a connection of its own to the
    /// display the app is on.
    pub fn capture(id: u32) -> Result<Picture, Error> {
        let failed = |e: &dyn std::fmt::Display| Error::new("capture_failed", e.to_string());
        let (conn, _) = x11rb::connect(None).map_err(|e| failed(&e))?;
        let geometry = conn
            .get_geometry(id)
            .map_err(|e| failed(&e))?
            .reply()
            .map_err(|e| failed(&e))?;
        let image = conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                id,
                0,
                0,
                geometry.width,
                geometry.height,
                !0,
            )
            .map_err(|e| failed(&e))?
            .reply()
            .map_err(|e| failed(&e))?;
        let (width, height) = (geometry.width as u32, geometry.height as u32);
        let pixels = (width * height) as usize;
        if !matches!(image.depth, 24 | 32) || image.data.len() < pixels * 4 {
            return Err(Error::new(
                "capture_unsupported",
                format!("the window's pixels are {} bits deep", image.depth),
            ));
        }
        let lsb = conn.setup().image_byte_order == ImageOrder::LSB_FIRST;
        let rgba = image.data[..pixels * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| match lsb {
                true => [p[2], p[1], p[0], 255],
                false => [p[1], p[2], p[3], 255],
            })
            .collect();
        Ok(Picture {
            rgba,
            width,
            height,
        })
    }
}

#[cfg(windows)]
mod win32 {
    use ahjo_app_kit::capture::Picture;
    use ahjo_core::Error;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ, ReleaseDC, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

    /// PW_CLIENTONLY and PW_RENDERFULLCONTENT: the client area as
    /// DirectComposition shows it, covered or off screen as well.
    const CLIENT_FULL_CONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(1 | 2);

    /// Window `hwnd`'s client area, printed into a bitmap of our own.
    pub fn capture(hwnd: isize) -> Result<Picture, Error> {
        let failed = |what: &str| Error::new("capture_failed", what.to_string());
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        let mut rect = RECT::default();
        // SAFETY: plain Win32 calls on the app's own window; every handle
        // made here is released before returning.
        unsafe {
            GetClientRect(hwnd, &mut rect).map_err(|e| failed(&e.to_string()))?;
            let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
            if width <= 0 || height <= 0 {
                return Err(failed("the window has no area"));
            }
            let screen = GetDC(Some(hwnd));
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            let printed = PrintWindow(hwnd, dc, CLIENT_FULL_CONTENT).as_bool();
            SelectObject(dc, old);
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    // Negative: top row first.
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; width as usize * height as usize * 4];
            let rows = GetDIBits(
                dc,
                bitmap,
                0,
                height as u32,
                Some(bgra.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            ReleaseDC(Some(hwnd), screen);
            if !printed {
                return Err(failed("PrintWindow refused the window"));
            }
            if rows != height {
                return Err(failed("the printed window did not read back"));
            }
            let rgba = bgra
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], 255])
                .collect();
            Ok(Picture {
                rgba,
                width: width as u32,
                height: height as u32,
            })
        }
    }
}
