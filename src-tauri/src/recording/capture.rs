use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Capture a specific window's frames using PrintWindow (with DWM content) + fallback to screen BitBlt.
///
/// GetDC(hwnd) + BitBlt does NOT work for GPU-accelerated windows (Chrome, Edge, etc.)
/// because their content is rendered via DirectComposition/Direct3D, not GDI.
/// PrintWindow with PW_RENDERFULLCONTENT (flag=2) asks DWM to render the composited content.
/// If that fails, we fall back to capturing from the desktop DC and cropping to window position.
#[cfg(windows)]
pub fn capture_window(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
    fps: u32,
    hwnd_raw: isize,
) -> Result<()> {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};

    /// PW_RENDERFULLCONTENT = 2: tells DWM to render the full composited visual content
    /// including DirectComposition and Direct3D surfaces.
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);

    log::info!("Window capture thread started (HWND: {}, {}fps)", hwnd_raw, fps);

    let frames_dir = output_dir.join("frames");
    std::fs::create_dir_all(&frames_dir)?;

    let frame_interval = std::time::Duration::from_nanos(1_000_000_000 / fps as u64);
    let mut frame_count: u64 = 0;

    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let mut last_buffer: Option<Vec<u8>> = None;
        let mut last_width: i32 = 0;
        let mut last_height: i32 = 0;

        // Keep a screen DC for the fallback path (reused across frames)
        let screen_dc = GetDC(HWND::default());

        while is_running.load(Ordering::SeqCst) {
            let frame_start = std::time::Instant::now();

            if is_paused.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }

            // Get current window rect
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                // Window may have been closed - reuse last frame or skip
                if let Some(ref buf) = last_buffer {
                    if last_width > 0 && last_height > 0 {
                        if let Some(img) = image::RgbaImage::from_raw(
                            last_width as u32, last_height as u32, buf.clone(),
                        ) {
                            let frame_path = frames_dir.join(format!("frame_{:08}.png", frame_count));
                            let _ = img.save(&frame_path);
                            frame_count += 1;
                        }
                    }
                }
                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    std::thread::sleep(frame_interval - elapsed);
                }
                continue;
            }

            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;

            if width <= 0 || height <= 0 {
                // Window is minimized - reuse last frame
                if let Some(ref buf) = last_buffer {
                    if last_width > 0 && last_height > 0 {
                        if let Some(img) = image::RgbaImage::from_raw(
                            last_width as u32, last_height as u32, buf.clone(),
                        ) {
                            let frame_path = frames_dir.join(format!("frame_{:08}.png", frame_count));
                            let _ = img.save(&frame_path);
                            frame_count += 1;
                        }
                    }
                }
                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    std::thread::sleep(frame_interval - elapsed);
                }
                continue;
            }

            // Save dimensions (update on resize)
            if width != last_width || height != last_height {
                let dims = format!("{}x{}", width, height);
                let _ = std::fs::write(output_dir.join("dimensions.txt"), &dims);
                last_width = width;
                last_height = height;
            }

            // Create memory DC and bitmap for capturing
            let mem_dc = CreateCompatibleDC(screen_dc);
            let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
            let old_bitmap = SelectObject(mem_dc, bitmap);

            // Strategy 1: PrintWindow with PW_RENDERFULLCONTENT
            // This asks DWM to render the full composited content (works for GPU-accelerated apps)
            let pw_ok = PrintWindow(hwnd, mem_dc, PW_RENDERFULLCONTENT);

            if !pw_ok.as_bool() {
                // Strategy 2: Capture from desktop DC and crop to window position
                // This always works because DWM has already composited all windows
                let _ = BitBlt(
                    mem_dc, 0, 0, width, height,
                    screen_dc, rect.left, rect.top, SRCCOPY,
                );
                if frame_count == 0 {
                    log::warn!("PrintWindow failed, falling back to screen BitBlt crop");
                }
            }

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // Top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: 0,
                    biSizeImage: (width * height * 4) as u32,
                    ..Default::default()
                },
                ..Default::default()
            };

            let buffer_size = (width * height * 4) as usize;
            let mut buffer = vec![0u8; buffer_size];

            GetDIBits(
                mem_dc, bitmap, 0, height as u32,
                Some(buffer.as_mut_ptr() as *mut _),
                &mut bmi, DIB_RGB_COLORS,
            );

            // BGRA → RGBA
            for chunk in buffer.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }

            // Cleanup GDI objects (but keep screen_dc alive)
            SelectObject(mem_dc, old_bitmap);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(mem_dc);

            // Save frame
            if let Some(img) = image::RgbaImage::from_raw(
                width as u32, height as u32, buffer.clone(),
            ) {
                let frame_path = frames_dir.join(format!("frame_{:08}.png", frame_count));
                let _ = img.save(&frame_path);
            }

            last_buffer = Some(buffer);
            frame_count += 1;

            let elapsed = frame_start.elapsed();
            if elapsed < frame_interval {
                std::thread::sleep(frame_interval - elapsed);
            }
        }

        // Release the screen DC
        ReleaseDC(HWND::default(), screen_dc);
    }

    log::info!("Window capture stopped. Total frames: {}", frame_count);
    std::fs::write(output_dir.join("frame_count.txt"), frame_count.to_string())?;

    Ok(())
}

#[cfg(not(windows))]
pub fn capture_window(
    _is_running: Arc<AtomicBool>,
    _is_paused: Arc<AtomicBool>,
    _output_dir: &Path,
    _fps: u32,
    _hwnd_raw: isize,
) -> Result<()> {
    Err(anyhow::anyhow!("Window capture is only supported on Windows"))
}

/// Capture a specific area of the screen using GDI BitBlt.
pub fn capture_area(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
    fps: u32,
    area_x: i32,
    area_y: i32,
    area_w: i32,
    area_h: i32,
) -> Result<()> {
    log::info!("Area capture started ({},{} {}x{}, {}fps)", area_x, area_y, area_w, area_h, fps);

    let frames_dir = output_dir.join("frames");
    std::fs::create_dir_all(&frames_dir)?;

    let frame_interval = std::time::Duration::from_nanos(1_000_000_000 / fps as u64);
    let mut frame_count: u64 = 0;

    // Save dimensions
    let dims = format!("{}x{}", area_w, area_h);
    std::fs::write(output_dir.join("dimensions.txt"), &dims)?;

    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::Foundation::*;

        unsafe {
            let screen_dc = GetDC(HWND::default());
            let mem_dc = CreateCompatibleDC(screen_dc);
            let bitmap = CreateCompatibleBitmap(screen_dc, area_w, area_h);
            let old_bitmap = SelectObject(mem_dc, bitmap);

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: area_w,
                    biHeight: -area_h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: 0,
                    biSizeImage: (area_w * area_h * 4) as u32,
                    ..Default::default()
                },
                ..Default::default()
            };

            let buffer_size = (area_w * area_h * 4) as usize;
            let mut buffer = vec![0u8; buffer_size];

            while is_running.load(Ordering::SeqCst) {
                let frame_start = std::time::Instant::now();

                if is_paused.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }

                let _ = BitBlt(mem_dc, 0, 0, area_w, area_h, screen_dc, area_x, area_y, SRCCOPY);

                GetDIBits(
                    mem_dc, bitmap, 0, area_h as u32,
                    Some(buffer.as_mut_ptr() as *mut _),
                    &mut bmi, DIB_RGB_COLORS,
                );

                for chunk in buffer.chunks_exact_mut(4) {
                    chunk.swap(0, 2);
                }

                if let Some(img) = image::RgbaImage::from_raw(
                    area_w as u32, area_h as u32, buffer.clone(),
                ) {
                    let frame_path = frames_dir.join(format!("frame_{:08}.png", frame_count));
                    let _ = img.save(&frame_path);
                }

                frame_count += 1;

                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    std::thread::sleep(frame_interval - elapsed);
                }
            }

            SelectObject(mem_dc, old_bitmap);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(HWND::default(), screen_dc);
        }
    }

    log::info!("Area capture stopped. Total frames: {}", frame_count);
    std::fs::write(output_dir.join("frame_count.txt"), frame_count.to_string())?;

    Ok(())
}

/// Capture screen frames using Windows GDI (BitBlt)
/// This is simpler and more compatible than Desktop Duplication API
pub fn capture_screen(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
    fps: u32,
) -> Result<()> {
    log::info!("Screen capture thread started (GDI mode, {}fps)", fps);

    let frames_dir = output_dir.join("frames");
    std::fs::create_dir_all(&frames_dir)?;

    let frame_interval = std::time::Duration::from_nanos(1_000_000_000 / fps as u64);
    let mut frame_count: u64 = 0;

    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::Win32::Foundation::*;

        unsafe {
            let screen_dc = GetDC(HWND::default());
            let width = GetSystemMetrics(SM_CXSCREEN);
            let height = GetSystemMetrics(SM_CYSCREEN);

            // Save dimensions
            let dims = format!("{}x{}", width, height);
            std::fs::write(output_dir.join("dimensions.txt"), &dims)?;

            let mem_dc = CreateCompatibleDC(screen_dc);
            let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
            let old_bitmap = SelectObject(mem_dc, bitmap);

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // Top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: 0, // BI_RGB
                    biSizeImage: (width * height * 4) as u32,
                    ..Default::default()
                },
                ..Default::default()
            };

            let buffer_size = (width * height * 4) as usize;
            let mut buffer = vec![0u8; buffer_size];

            while is_running.load(Ordering::SeqCst) {
                let frame_start = std::time::Instant::now();

                if is_paused.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }

                // Capture screen
                let _ = BitBlt(mem_dc, 0, 0, width, height, screen_dc, 0, 0, SRCCOPY);

                // Get pixel data
                GetDIBits(
                    mem_dc,
                    bitmap,
                    0,
                    height as u32,
                    Some(buffer.as_mut_ptr() as *mut _),
                    &mut bmi,
                    DIB_RGB_COLORS,
                );

                // Convert BGRA to RGBA
                for chunk in buffer.chunks_exact_mut(4) {
                    chunk.swap(0, 2);
                }

                // Save frame as PNG (for FFmpeg later)
                if let Some(img) = image::RgbaImage::from_raw(
                    width as u32,
                    height as u32,
                    buffer.clone(),
                ) {
                    let frame_path = frames_dir.join(format!("frame_{:08}.png", frame_count));
                    let _ = img.save(&frame_path);
                }

                frame_count += 1;

                // Maintain frame rate
                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    std::thread::sleep(frame_interval - elapsed);
                }
            }

            // Cleanup
            SelectObject(mem_dc, old_bitmap);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(HWND::default(), screen_dc);
        }
    }

    log::info!("Screen capture stopped. Total frames: {}", frame_count);

    std::fs::write(
        output_dir.join("frame_count.txt"),
        frame_count.to_string(),
    )?;

    Ok(())
}
