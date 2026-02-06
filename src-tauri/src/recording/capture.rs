use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
