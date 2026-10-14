//! 録画中はメインウィンドウを画面上部中央の小さな録画バーにする (Issue #1)。
//!
//! バーは最前面・枠なしにし、`WDA_EXCLUDEFROMCAPTURE` で録画に映らないようにする。
//! 録画を終えたら元の大きさ・位置に戻す。

use std::sync::Mutex;
use tauri::{LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow};

/// 録画バーの大きさ（論理 px）。`src/components/RecordingBar.tsx` の内容が収まる大きさ
const BAR_WIDTH: f64 = 360.0;
const BAR_HEIGHT: f64 = 48.0;
/// 画面上端からの距離（論理 px）
const BAR_TOP_MARGIN: f64 = 16.0;
/// 通常時の最小サイズ（tauri.conf.json の minWidth / minHeight と揃える）
const MIN_WIDTH: f64 = 680.0;
const MIN_HEIGHT: f64 = 500.0;

struct Saved {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    maximized: bool,
}

/// 録画バーにする前のウィンドウの位置・大きさ
#[derive(Default)]
pub struct WindowBarState(Mutex<Option<Saved>>);

pub fn enter(window: &WebviewWindow, state: &WindowBarState) -> tauri::Result<()> {
    {
        let mut saved = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if saved.is_none() {
            *saved = Some(Saved {
                position: window.outer_position()?,
                size: window.inner_size()?,
                maximized: window.is_maximized()?,
            });
        }
    }
    // 録画が始まる前に映らないようにしておく
    exclude_from_capture(window, true);
    if window.is_maximized()? {
        window.unmaximize()?;
    }
    window.set_min_size(None::<LogicalSize<f64>>)?;
    window.set_decorations(false)?;
    window.set_resizable(false)?;
    window.set_always_on_top(true)?;
    window.set_size(LogicalSize::new(BAR_WIDTH, BAR_HEIGHT))?;
    if let Some(monitor) = window.current_monitor()? {
        let scale = monitor.scale_factor();
        let width = (BAR_WIDTH * scale).round() as i32;
        // 枠なしでもウィンドウの外形には見えない縁が含まれるので、その分ずらして中身を中央に置く
        let (outer, inner) = (window.outer_position()?, window.inner_position()?);
        let (dx, dy) = (inner.x - outer.x, inner.y - outer.y);
        let x = monitor.position().x + (monitor.size().width as i32 - width) / 2 - dx;
        let y = monitor.position().y + (BAR_TOP_MARGIN * scale).round() as i32 - dy;
        window.set_position(PhysicalPosition::new(x, y))?;
    }
    window.show()?;
    Ok(())
}

pub fn exit(window: &WebviewWindow, state: &WindowBarState) -> tauri::Result<()> {
    let saved = state.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    exclude_from_capture(window, false);
    window.set_always_on_top(false)?;
    window.set_decorations(true)?;
    window.set_resizable(true)?;
    window.set_min_size(Some(LogicalSize::new(MIN_WIDTH, MIN_HEIGHT)))?;
    match saved {
        Some(saved) => {
            window.set_size(saved.size)?;
            window.set_position(saved.position)?;
            if saved.maximized {
                window.maximize()?;
            }
        }
        None => {
            window.set_size(LogicalSize::new(880.0, 600.0))?;
            window.center()?;
        }
    }
    window.show()?;
    window.set_focus()?;
    Ok(())
}

#[cfg(windows)]
fn exclude_from_capture(window: &WebviewWindow, exclude: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };
    let Ok(hwnd) = window.hwnd() else { return };
    let affinity = if exclude { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    if let Err(e) = unsafe { SetWindowDisplayAffinity(HWND(hwnd.0 as _), affinity) } {
        log::warn!("SetWindowDisplayAffinity failed: {}", e);
    }
}

#[cfg(not(windows))]
fn exclude_from_capture(_window: &WebviewWindow, _exclude: bool) {}
