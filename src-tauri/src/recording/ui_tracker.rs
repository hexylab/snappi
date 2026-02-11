use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Track UI Automation focus change events and write them to ui_events.jsonl.
/// Runs on a dedicated STA thread (required for COM UI Automation).
/// If UI Automation is unavailable, this function returns without error.
pub fn track_ui_events(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
) -> Result<()> {
    #[cfg(windows)]
    {
        track_ui_events_windows(is_running, is_paused, output_dir)
    }

    #[cfg(not(windows))]
    {
        let _ = (is_running, is_paused, output_dir);
        Ok(())
    }
}

#[cfg(windows)]
fn track_ui_events_windows(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    output_dir: &Path,
) -> Result<()> {
    use std::io::Write;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Accessibility::*;

    log::info!("UI tracker thread started");

    let events_path = output_dir.join("ui_events.jsonl");
    let file = std::sync::Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)?,
    );
    let start_time = std::time::Instant::now();

    // Initialize COM in STA mode (required for UI Automation event handlers)
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() {
            log::warn!("COM initialization failed for UI tracker: {:?}", hr);
            return Ok(());
        }
    }

    // Create UI Automation instance
    let automation: IUIAutomation = unsafe {
        match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
            Ok(a) => a,
            Err(e) => {
                log::warn!("Failed to create IUIAutomation: {:?}", e);
                CoUninitialize();
                return Ok(());
            }
        }
    };

    // Poll-based approach: periodically check focused element
    // This avoids the complexity of implementing COM event handler interfaces
    let mut last_element_name = String::new();
    let mut last_event_ms: u64 = 0;
    let debounce_ms: u64 = 100;

    while is_running.load(Ordering::SeqCst) {
        if is_paused.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        }

        let elapsed_ms = start_time.elapsed().as_millis() as u64;

        // Debounce: skip if too soon
        if elapsed_ms.saturating_sub(last_event_ms) < debounce_ms {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }

        // Get the currently focused element
        if let Ok(element) = unsafe { automation.GetFocusedElement() } {
            let name = unsafe { element.CurrentName() }
                .map(|s| s.to_string())
                .unwrap_or_default();

            let control_type = unsafe { element.CurrentControlType() }.unwrap_or(UIA_CustomControlTypeId);
            let control_name = control_type_name(control_type);

            let automation_id = unsafe { element.CurrentAutomationId() }
                .map(|s| s.to_string())
                .unwrap_or_default();

            // Only emit if the focused element actually changed
            let element_key = format!("{}:{}:{}", control_name, name, automation_id);
            if element_key != last_element_name {
                last_element_name = element_key;
                last_event_ms = elapsed_ms;

                // Get bounding rectangle
                let rect = unsafe { element.CurrentBoundingRectangle() }
                    .unwrap_or_default();
                let rect_arr = [
                    rect.left as f64,
                    rect.top as f64,
                    rect.right as f64,
                    rect.bottom as f64,
                ];

                // Determine event type based on control type
                let is_menu = control_type == UIA_MenuControlTypeId
                    || control_type == UIA_MenuItemControlTypeId
                    || control_type == UIA_MenuBarControlTypeId;
                let is_window_like = control_type == UIA_WindowControlTypeId
                    || control_type == UIA_PaneControlTypeId;

                let event_type = if is_menu {
                    "ui_menu_open"
                } else if is_window_like {
                    let w = (rect.right - rect.left) as i32;
                    let h = (rect.bottom - rect.top) as i32;
                    if w < 800 && h < 600 && w > 50 && h > 50 {
                        "ui_dialog_open"
                    } else {
                        "ui_focus"
                    }
                } else {
                    "ui_focus"
                };

                // Write JSON line
                let json = match event_type {
                    "ui_menu_open" => {
                        format!(
                            r#"{{"type":"ui_menu_open","t":{},"control":"{}","name":"{}","rect":[{},{},{},{}]}}"#,
                            elapsed_ms, control_name, escape_json(&name),
                            rect_arr[0], rect_arr[1], rect_arr[2], rect_arr[3],
                        )
                    }
                    "ui_dialog_open" => {
                        format!(
                            r#"{{"type":"ui_dialog_open","t":{},"control":"{}","name":"{}","rect":[{},{},{},{}]}}"#,
                            elapsed_ms, control_name, escape_json(&name),
                            rect_arr[0], rect_arr[1], rect_arr[2], rect_arr[3],
                        )
                    }
                    _ => {
                        format!(
                            r#"{{"type":"ui_focus","t":{},"control":"{}","name":"{}","rect":[{},{},{},{}],"automation_id":"{}"}}"#,
                            elapsed_ms, control_name, escape_json(&name),
                            rect_arr[0], rect_arr[1], rect_arr[2], rect_arr[3],
                            escape_json(&automation_id),
                        )
                    }
                };

                if let Ok(mut f) = file.lock() {
                    let _ = writeln!(f, "{}", json);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    unsafe { CoUninitialize(); }
    log::info!("UI tracker thread stopped");
    Ok(())
}

#[cfg(windows)]
fn control_type_name(ct: windows::Win32::UI::Accessibility::UIA_CONTROLTYPE_ID) -> &'static str {
    use windows::Win32::UI::Accessibility::*;
    // UIA_CONTROLTYPE_ID are i32 constants; use if-else chain to avoid match binding issues
    if ct == UIA_ButtonControlTypeId { "Button" }
    else if ct == UIA_CalendarControlTypeId { "Calendar" }
    else if ct == UIA_CheckBoxControlTypeId { "CheckBox" }
    else if ct == UIA_ComboBoxControlTypeId { "ComboBox" }
    else if ct == UIA_EditControlTypeId { "Edit" }
    else if ct == UIA_HyperlinkControlTypeId { "Hyperlink" }
    else if ct == UIA_ImageControlTypeId { "Image" }
    else if ct == UIA_ListItemControlTypeId { "ListItem" }
    else if ct == UIA_ListControlTypeId { "List" }
    else if ct == UIA_MenuControlTypeId { "Menu" }
    else if ct == UIA_MenuBarControlTypeId { "MenuBar" }
    else if ct == UIA_MenuItemControlTypeId { "MenuItem" }
    else if ct == UIA_ProgressBarControlTypeId { "ProgressBar" }
    else if ct == UIA_RadioButtonControlTypeId { "RadioButton" }
    else if ct == UIA_ScrollBarControlTypeId { "ScrollBar" }
    else if ct == UIA_SliderControlTypeId { "Slider" }
    else if ct == UIA_SpinnerControlTypeId { "Spinner" }
    else if ct == UIA_StatusBarControlTypeId { "StatusBar" }
    else if ct == UIA_TabControlTypeId { "Tab" }
    else if ct == UIA_TabItemControlTypeId { "TabItem" }
    else if ct == UIA_TextControlTypeId { "Text" }
    else if ct == UIA_ToolBarControlTypeId { "ToolBar" }
    else if ct == UIA_ToolTipControlTypeId { "ToolTip" }
    else if ct == UIA_TreeControlTypeId { "Tree" }
    else if ct == UIA_TreeItemControlTypeId { "TreeItem" }
    else if ct == UIA_GroupControlTypeId { "Group" }
    else if ct == UIA_ThumbControlTypeId { "Thumb" }
    else if ct == UIA_DataGridControlTypeId { "DataGrid" }
    else if ct == UIA_DataItemControlTypeId { "DataItem" }
    else if ct == UIA_DocumentControlTypeId { "Document" }
    else if ct == UIA_SplitButtonControlTypeId { "SplitButton" }
    else if ct == UIA_WindowControlTypeId { "Window" }
    else if ct == UIA_PaneControlTypeId { "Pane" }
    else if ct == UIA_HeaderControlTypeId { "Header" }
    else if ct == UIA_HeaderItemControlTypeId { "HeaderItem" }
    else if ct == UIA_TableControlTypeId { "Table" }
    else if ct == UIA_TitleBarControlTypeId { "TitleBar" }
    else if ct == UIA_SeparatorControlTypeId { "Separator" }
    else { "Custom" }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
