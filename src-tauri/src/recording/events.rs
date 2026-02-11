use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

// Modifier bit flags
const MOD_CTRL: u8 = 0b0001;
const MOD_SHIFT: u8 = 0b0010;
const MOD_ALT: u8 = 0b0100;
const MOD_META: u8 = 0b1000;

fn modifiers_from_flags(flags: u8) -> Vec<String> {
    let mut mods = Vec::new();
    if flags & MOD_CTRL != 0 {
        mods.push("Ctrl".to_string());
    }
    if flags & MOD_SHIFT != 0 {
        mods.push("Shift".to_string());
    }
    if flags & MOD_ALT != 0 {
        mods.push("Alt".to_string());
    }
    if flags & MOD_META != 0 {
        mods.push("Win".to_string());
    }
    mods
}

/// Windows低レベルフック実装
/// rdevの代わりにSetWindowsHookExWを直接使用し、
/// クリック重複・欠落問題を解決する
#[cfg(windows)]
mod win_hooks {
    use crate::config::RecordingEvent;
    use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Instant;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::*;

    use super::{modifiers_from_flags, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT};

    /// フックコールバックからアクセスするグローバル共有状態
    static HOOK_STATE: std::sync::Mutex<Option<Arc<HookSharedState>>> =
        std::sync::Mutex::new(None);

    pub struct HookSharedState {
        pub events: std::sync::Mutex<Vec<RecordingEvent>>,
        pub start_time: Instant,
        pub last_mouse_time: std::sync::Mutex<Instant>,
        pub modifier_state: AtomicU8,
        pub is_running: Arc<AtomicBool>,
        pub is_paused: Arc<AtomicBool>,
        /// クリック重複排除用: 直前のクリック時刻(ms)
        pub last_click_time: AtomicU64,
        /// クリック重複排除用: 直前のクリックボタン名
        pub last_click_btn: std::sync::Mutex<String>,
    }

    pub fn set_state(state: Arc<HookSharedState>) {
        *HOOK_STATE.lock().unwrap() = Some(state);
    }

    pub fn clear_state() {
        *HOOK_STATE.lock().unwrap() = None;
    }

    fn with_state<F: FnOnce(&HookSharedState)>(f: F) {
        if let Ok(guard) = HOOK_STATE.lock() {
            if let Some(state) = guard.as_ref() {
                f(state);
            }
        }
    }

    // ─── マウスフック ───

    pub unsafe extern "system" fn mouse_hook_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code >= 0 {
            with_state(|state| {
                if !state.is_running.load(Ordering::SeqCst)
                    || state.is_paused.load(Ordering::SeqCst)
                {
                    return;
                }

                let mouse = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                let t = state.start_time.elapsed().as_millis() as u64;
                let x = mouse.pt.x as f64;
                let y = mouse.pt.y as f64;

                let event = match wparam.0 as u32 {
                    WM_MOUSEMOVE => {
                        let mut last = state.last_mouse_time.lock().unwrap();
                        if last.elapsed().as_millis() >= 10 {
                            *last = Instant::now();
                            Some(RecordingEvent::MouseMove { t, x, y })
                        } else {
                            None
                        }
                    }
                    WM_LBUTTONDOWN => make_click(state, t, "left", x, y),
                    WM_RBUTTONDOWN => make_click(state, t, "right", x, y),
                    WM_MBUTTONDOWN => make_click(state, t, "middle", x, y),
                    WM_LBUTTONUP => Some(RecordingEvent::ClickRelease {
                        t,
                        btn: "left".into(),
                        x,
                        y,
                    }),
                    WM_RBUTTONUP => Some(RecordingEvent::ClickRelease {
                        t,
                        btn: "right".into(),
                        x,
                        y,
                    }),
                    WM_MBUTTONUP => Some(RecordingEvent::ClickRelease {
                        t,
                        btn: "middle".into(),
                        x,
                        y,
                    }),
                    WM_MOUSEWHEEL => {
                        let delta = (mouse.mouseData >> 16) as i16;
                        Some(RecordingEvent::Scroll {
                            t,
                            x,
                            y,
                            dx: 0.0,
                            dy: delta as f64 / 120.0,
                        })
                    }
                    0x020E /* WM_MOUSEHWHEEL */ => {
                        let delta = (mouse.mouseData >> 16) as i16;
                        Some(RecordingEvent::Scroll {
                            t,
                            x,
                            y,
                            dx: delta as f64 / 120.0,
                            dy: 0.0,
                        })
                    }
                    _ => None,
                };

                if let Some(evt) = event {
                    if let Ok(mut buf) = state.events.lock() {
                        buf.push(evt);
                    }
                }
            });
        }
        CallNextHookEx(HHOOK::default(), code, wparam, lparam)
    }

    /// クリックイベント生成（重複排除付き）
    /// 同一ボタンが20ms以内に再度発生した場合はスキップ
    fn make_click(
        state: &HookSharedState,
        t: u64,
        btn: &str,
        x: f64,
        y: f64,
    ) -> Option<RecordingEvent> {
        let prev_t = state.last_click_time.load(Ordering::SeqCst);
        if t.saturating_sub(prev_t) < 20 {
            if let Ok(prev_btn) = state.last_click_btn.lock() {
                if *prev_btn == btn {
                    return None;
                }
            }
        }
        state.last_click_time.store(t, Ordering::SeqCst);
        if let Ok(mut prev_btn) = state.last_click_btn.lock() {
            prev_btn.clear();
            prev_btn.push_str(btn);
        }
        Some(RecordingEvent::Click {
            t,
            btn: btn.to_string(),
            x,
            y,
        })
    }

    // ─── キーボードフック ───

    pub unsafe extern "system" fn keyboard_hook_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code >= 0 {
            with_state(|state| {
                if !state.is_running.load(Ordering::SeqCst)
                    || state.is_paused.load(Ordering::SeqCst)
                {
                    return;
                }

                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                let t = state.start_time.elapsed().as_millis() as u64;
                let vk = kb.vkCode;

                match wparam.0 as u32 {
                    WM_KEYDOWN | WM_SYSKEYDOWN => {
                        if let Some(flag) = vk_to_modifier(vk) {
                            state.modifier_state.fetch_or(flag, Ordering::SeqCst);
                        } else {
                            let key_name = vk_to_name(vk);
                            let flags = state.modifier_state.load(Ordering::SeqCst);
                            let modifiers = modifiers_from_flags(flags);
                            if let Ok(mut buf) = state.events.lock() {
                                buf.push(RecordingEvent::Key {
                                    t,
                                    key: key_name,
                                    modifiers,
                                });
                            }
                        }
                    }
                    WM_KEYUP | WM_SYSKEYUP => {
                        if let Some(flag) = vk_to_modifier(vk) {
                            state.modifier_state.fetch_and(!flag, Ordering::SeqCst);
                        }
                    }
                    _ => {}
                }
            });
        }
        CallNextHookEx(HHOOK::default(), code, wparam, lparam)
    }

    fn vk_to_modifier(vk: u32) -> Option<u8> {
        match vk {
            0x10 | 0xA0 | 0xA1 => Some(MOD_SHIFT),   // VK_SHIFT / VK_LSHIFT / VK_RSHIFT
            0x11 | 0xA2 | 0xA3 => Some(MOD_CTRL),     // VK_CONTROL / VK_LCONTROL / VK_RCONTROL
            0x12 | 0xA4 | 0xA5 => Some(MOD_ALT),      // VK_MENU / VK_LMENU / VK_RMENU
            0x5B | 0x5C => Some(MOD_META),             // VK_LWIN / VK_RWIN
            _ => None,
        }
    }

    fn vk_to_name(vk: u32) -> String {
        match vk {
            0x08 => "Backspace".into(),
            0x09 => "Tab".into(),
            0x0D => "Return".into(),
            0x13 => "Pause".into(),
            0x14 => "CapsLock".into(),
            0x1B => "Escape".into(),
            0x20 => "Space".into(),
            0x21 => "PageUp".into(),
            0x22 => "PageDown".into(),
            0x23 => "End".into(),
            0x24 => "Home".into(),
            0x25 => "Left".into(),
            0x26 => "Up".into(),
            0x27 => "Right".into(),
            0x28 => "Down".into(),
            0x2C => "PrintScreen".into(),
            0x2D => "Insert".into(),
            0x2E => "Delete".into(),
            0x30..=0x39 => format!("{}", vk - 0x30),
            0x41..=0x5A => format!("{}", (vk as u8) as char),
            0x60..=0x69 => format!("Num{}", vk - 0x60),
            0x6A => "NumMultiply".into(),
            0x6B => "NumAdd".into(),
            0x6D => "NumSubtract".into(),
            0x6E => "NumDecimal".into(),
            0x6F => "NumDivide".into(),
            0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
            0x90 => "NumLock".into(),
            0x91 => "ScrollLock".into(),
            0xBA => "Semicolon".into(),
            0xBB => "Equal".into(),
            0xBC => "Comma".into(),
            0xBD => "Minus".into(),
            0xBE => "Period".into(),
            0xBF => "Slash".into(),
            0xC0 => "BackQuote".into(),
            0xDB => "BracketLeft".into(),
            0xDC => "Backslash".into(),
            0xDD => "BracketRight".into(),
            0xDE => "Quote".into(),
            _ => format!("VK_{:#04X}", vk),
        }
    }
}

pub fn collect_events(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
) -> Result<()> {
    let events_path = output_dir.join("events.jsonl");
    let mut file = std::fs::File::create(&events_path)?;

    log::info!("Event collection thread started");

    #[cfg(windows)]
    {
        use std::sync::atomic::{AtomicU32, AtomicU64};
        use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::*;

        let shared = Arc::new(win_hooks::HookSharedState {
            events: std::sync::Mutex::new(Vec::new()),
            start_time: Instant::now(),
            last_mouse_time: std::sync::Mutex::new(Instant::now()),
            modifier_state: AtomicU8::new(0),
            is_running: is_running.clone(),
            is_paused: is_paused.clone(),
            last_click_time: AtomicU64::new(u64::MAX),
            last_click_btn: std::sync::Mutex::new(String::new()),
        });

        // グローバル状態にセット（フックコールバックからアクセス用）
        win_hooks::set_state(shared.clone());

        // フックスレッドのWindows Thread ID（WM_QUIT送信用）
        let hook_tid = Arc::new(AtomicU32::new(0));
        let hook_tid_clone = hook_tid.clone();

        // フック準備完了シグナル
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();

        let hook_handle = std::thread::spawn(move || unsafe {
            // スレッドIDを保存（停止時のPostThreadMessageW用）
            hook_tid_clone.store(GetCurrentThreadId(), Ordering::SeqCst);

            let mouse_hook = SetWindowsHookExW(
                WH_MOUSE_LL,
                Some(win_hooks::mouse_hook_proc),
                HINSTANCE::default(),
                0,
            );
            let kb_hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(win_hooks::keyboard_hook_proc),
                HINSTANCE::default(),
                0,
            );

            if mouse_hook.is_err() {
                log::error!("マウスフックのインストールに失敗");
            }
            if kb_hook.is_err() {
                log::error!("キーボードフックのインストールに失敗");
            }

            // フック準備完了を通知
            let _ = ready_tx.send(());

            // メッセージポンプ（低レベルフックの動作に必須）
            let mut msg = MSG::default();
            loop {
                let ret = GetMessageW(&mut msg, HWND::default(), 0, 0);
                if ret.0 <= 0 {
                    break; // WM_QUIT (0) またはエラー (-1)
                }
            }

            // フック解除
            if let Ok(h) = mouse_hook {
                let _ = UnhookWindowsHookEx(h);
            }
            if let Ok(h) = kb_hook {
                let _ = UnhookWindowsHookEx(h);
            }
        });

        // フック準備完了を待機
        let _ = ready_rx.recv();

        // イベントバッファを定期的にファイルへフラッシュ
        while is_running.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));

            let mut buffer = shared.events.lock().unwrap();
            for event in buffer.drain(..) {
                if let Ok(json) = serde_json::to_string(&event) {
                    let _ = writeln!(file, "{}", json);
                }
            }
            let _ = file.flush();
        }

        // フックスレッドを停止（WM_QUITを送信）
        unsafe {
            let tid = hook_tid.load(Ordering::SeqCst);
            if tid != 0 {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }

        // 最終フラッシュ
        if let Ok(mut buffer) = shared.events.lock() {
            for event in buffer.drain(..) {
                if let Ok(json) = serde_json::to_string(&event) {
                    let _ = writeln!(file, "{}", json);
                }
            }
        }

        // グローバル状態をクリア
        win_hooks::clear_state();

        let _ = hook_handle.join();
    }

    #[cfg(not(windows))]
    {
        // Windows以外ではイベント収集なし
        while is_running.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    log::info!("Event collection stopped");
    Ok(())
}
