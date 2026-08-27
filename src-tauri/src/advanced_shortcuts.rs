use crate::desktop_lifecycle::{ShortcutAction, ShortcutPhase};
use crate::settings::ShortcutBinding;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutRecordingAction {
    QuickQuery,
    SelectionExplanation,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutRecordingResult {
    action: ShortcutRecordingAction,
    binding: Option<ShortcutBinding>,
    cancelled: bool,
    error: Option<String>,
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use tauri::{Emitter, Manager};
    use webview2_com::{
        AcceleratorKeyPressedEventHandler,
        Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP,
        },
    };
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::{
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
            SC_KEYMENU, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSCOMMAND, WM_SYSKEYDOWN,
            WM_SYSKEYUP,
        },
    };

    const DOUBLE_TAP_INTERVAL: Duration = Duration::from_millis(350);
    const SYSTEM_MENU_SUPPRESSION_INTERVAL: Duration = Duration::from_millis(500);
    const SHORTCUT_RECORDING_SUBCLASS_ID: usize = 0x5244_5259;
    const VK_SHIFT: u32 = 0x10;
    const VK_CONTROL: u32 = 0x11;
    const VK_MENU: u32 = 0x12;
    const VK_ESCAPE: u32 = 0x1B;
    const VK_LMENU: u32 = 0xA4;
    const VK_RMENU: u32 = 0xA5;
    const VK_LCONTROL: u32 = 0xA2;
    const VK_RCONTROL: u32 = 0xA3;
    const VK_LSHIFT: u32 = 0xA0;
    const VK_RSHIFT: u32 = 0xA1;
    const VK_LWIN: u32 = 0x5B;
    const VK_RWIN: u32 = 0x5C;

    enum EngineEvent {
        Trigger(ShortcutAction, ShortcutPhase),
        Recorded(ShortcutRecordingResult),
    }

    #[derive(Default)]
    struct DoubleTapTracker {
        last_release: Option<Instant>,
        second_down: bool,
    }

    impl DoubleTapTracker {
        fn reset(&mut self) {
            self.last_release = None;
            self.second_down = false;
        }

        fn update(
            &mut self,
            virtual_key: u32,
            key_down: bool,
            repeat: bool,
            now: Instant,
        ) -> Option<ShortcutPhase> {
            if virtual_key != VK_LMENU {
                if key_down {
                    self.reset();
                }
                return None;
            }
            if key_down {
                if repeat {
                    return None;
                }
                self.second_down = self
                    .last_release
                    .is_some_and(|released| now.duration_since(released) <= DOUBLE_TAP_INTERVAL);
                self.second_down.then_some(ShortcutPhase::Pressed)
            } else if self.second_down {
                self.reset();
                Some(ShortcutPhase::Released)
            } else {
                self.last_release = Some(now);
                None
            }
        }
    }

    struct RecordingState {
        action: ShortcutRecordingAction,
        tracker: DoubleTapTracker,
    }

    #[derive(Default)]
    struct HookState {
        runtime_action: Option<ShortcutAction>,
        runtime_tracker: DoubleTapTracker,
        recording: Option<RecordingState>,
        pressed: HashSet<u32>,
    }

    struct Engine {
        state: Mutex<HookState>,
        sender: mpsc::Sender<EngineEvent>,
        installed: AtomicBool,
        start_lock: Mutex<()>,
    }

    static ENGINE: OnceLock<Engine> = OnceLock::new();
    static SYSTEM_MENU_SUPPRESS_UNTIL: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

    fn system_menu_suppression_deadline() -> &'static Mutex<Option<Instant>> {
        SYSTEM_MENU_SUPPRESS_UNTIL.get_or_init(|| Mutex::new(None))
    }

    fn arm_system_menu_suppression() {
        if let Ok(mut deadline) = system_menu_suppression_deadline().lock() {
            *deadline = Some(Instant::now() + SYSTEM_MENU_SUPPRESSION_INTERVAL);
        }
    }

    fn clear_system_menu_suppression() {
        if let Ok(mut deadline) = system_menu_suppression_deadline().lock() {
            *deadline = None;
        }
    }

    fn recording_or_system_menu_suppression_active() -> bool {
        let recording = ENGINE
            .get()
            .and_then(|engine| engine.state.lock().ok())
            .is_some_and(|state| state.recording.is_some());
        if recording {
            return true;
        }
        system_menu_suppression_deadline()
            .lock()
            .ok()
            .and_then(|deadline| *deadline)
            .is_some_and(|deadline| Instant::now() <= deadline)
    }

    fn is_system_menu_command(message: u32, wparam: WPARAM) -> bool {
        message == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) as u32 == SC_KEYMENU
    }

    unsafe extern "system" fn shortcut_recording_window_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        _reference_data: usize,
    ) -> LRESULT {
        if is_system_menu_command(message, wparam) && recording_or_system_menu_suppression_active()
        {
            return LRESULT(0);
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    fn engine(app: &AppHandle) -> &'static Engine {
        ENGINE.get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<EngineEvent>();
            let app = app.clone();
            std::thread::spawn(move || {
                while let Ok(event) = receiver.recv() {
                    match event {
                        EngineEvent::Trigger(action, phase) => {
                            crate::handle_shortcut_action(&app, action, phase)
                        }
                        EngineEvent::Recorded(result) => {
                            if let Err(error) = app.emit("readray://shortcut-recorded", result) {
                                eprintln!("READRAY_SHORTCUT_RECORDING_EMIT_ERROR={error}");
                            }
                        }
                    }
                }
            });
            Engine {
                state: Mutex::new(HookState::default()),
                sender,
                installed: AtomicBool::new(false),
                start_lock: Mutex::new(()),
            }
        })
    }

    fn ensure_started(app: &AppHandle) -> Result<(), String> {
        let engine = engine(app);
        if engine.installed.load(Ordering::Acquire) {
            return Ok(());
        }
        let _guard = engine
            .start_lock
            .lock()
            .map_err(|error| error.to_string())?;
        if engine.installed.load(Ordering::Acquire) {
            return Ok(());
        }
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) };
            match hook {
                Ok(_hook) => {
                    if let Some(engine) = ENGINE.get() {
                        engine.installed.store(true, Ordering::Release);
                    }
                    let _ = ready_tx.send(Ok(()));
                    let mut message = MSG::default();
                    loop {
                        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
                        if result <= 0 {
                            break;
                        }
                    }
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("Windows 低级键盘监听启动失败：{error}")));
                }
            }
        });
        ready_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Windows 低级键盘监听启动超时。".to_string())?
    }

    unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let message = wparam.0 as u32;
            let key_down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            let key_up = message == WM_KEYUP || message == WM_SYSKEYUP;
            if key_down || key_up {
                let input = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
                if !input.flags.contains(LLKHF_INJECTED) {
                    if let Some(engine) = ENGINE.get() {
                        if process_input(engine, input.vkCode, key_down, false) {
                            return LRESULT(1);
                        }
                    }
                }
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    fn webview_key_down(kind: COREWEBVIEW2_KEY_EVENT_KIND) -> Option<bool> {
        match kind {
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN => {
                Some(true)
            }
            COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP => {
                Some(false)
            }
            _ => None,
        }
    }

    fn normalize_webview_virtual_key(virtual_key: u32, key_event_lparam: i32) -> u32 {
        let lparam = key_event_lparam as u32;
        let is_extended = lparam & (1 << 24) != 0;
        let scan_code = (lparam >> 16) & 0xFF;
        match virtual_key {
            VK_SHIFT if scan_code == 0x36 => VK_RSHIFT,
            VK_SHIFT => VK_LSHIFT,
            VK_CONTROL if is_extended => VK_RCONTROL,
            VK_CONTROL => VK_LCONTROL,
            VK_MENU if is_extended => VK_RMENU,
            VK_MENU => VK_LMENU,
            _ => virtual_key,
        }
    }

    pub(super) fn install_webview_accelerator_recording(
        app: &AppHandle,
        webview_label: &str,
    ) -> Result<(), String> {
        let webview = app
            .get_webview_window(webview_label)
            .ok_or_else(|| format!("找不到用于快捷键录制的 WebView：{webview_label}"))?;
        let hwnd = webview
            .hwnd()
            .map_err(|error| format!("无法获取快捷键录制窗口句柄：{error}"))?;
        let subclass_installed = unsafe {
            SetWindowSubclass(
                hwnd,
                Some(shortcut_recording_window_subclass),
                SHORTCUT_RECORDING_SUBCLASS_ID,
                0,
            )
        };
        if !subclass_installed.as_bool() {
            return Err(format!(
                "快捷键录制的系统菜单拦截安装失败：{}",
                windows::core::Error::from_win32()
            ));
        }

        webview
            .with_webview(|platform_webview| {
                let controller = platform_webview.controller();
                let handler = AcceleratorKeyPressedEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                    let mut virtual_key = 0;
                    let mut key_event_lparam = 0;
                    unsafe {
                        args.KeyEventKind(&mut kind)?;
                        args.VirtualKey(&mut virtual_key)?;
                        args.KeyEventLParam(&mut key_event_lparam)?;
                    }
                    let Some(key_down) = webview_key_down(kind) else {
                        return Ok(());
                    };
                    let virtual_key = normalize_webview_virtual_key(virtual_key, key_event_lparam);
                    let handled = ENGINE
                        .get()
                        .is_some_and(|engine| process_input(engine, virtual_key, key_down, true));
                    if handled {
                        if matches!(
                            kind,
                            COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                                | COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP
                        ) {
                            arm_system_menu_suppression();
                        }
                        unsafe {
                            args.SetHandled(true)?;
                        }
                    }
                    Ok(())
                }));
                let mut token = 0;
                if let Err(error) =
                    unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) }
                {
                    eprintln!("READRAY_SHORTCUT_ACCELERATOR_INSTALL_ERROR={error}");
                }
            })
            .map_err(|error| format!("快捷键录制的 WebView2 监听安装失败：{error}"))
    }

    fn process_input(
        engine: &Engine,
        virtual_key: u32,
        key_down: bool,
        recording_only: bool,
    ) -> bool {
        let now = Instant::now();
        let Ok(mut state) = engine.state.lock() else {
            return false;
        };
        if recording_only && state.recording.is_none() {
            return false;
        }
        let repeat = if key_down {
            !state.pressed.insert(virtual_key)
        } else {
            state.pressed.remove(&virtual_key);
            false
        };

        let regular_key_down = key_down && !repeat && !is_modifier(virtual_key);
        let recorded_chord = regular_key_down
            .then(|| chord_accelerator(&state.pressed, virtual_key))
            .flatten();
        let mut recording_finished = false;
        if let Some(recording) = state.recording.as_mut() {
            if key_down && !repeat && virtual_key == VK_ESCAPE {
                recording_finished = true;
                let _ = engine
                    .sender
                    .send(EngineEvent::Recorded(ShortcutRecordingResult {
                        action: recording.action,
                        binding: None,
                        cancelled: true,
                        error: None,
                    }));
            } else if recording.tracker.update(virtual_key, key_down, repeat, now)
                == Some(ShortcutPhase::Released)
            {
                recording_finished = true;
                let _ = engine
                    .sender
                    .send(EngineEvent::Recorded(ShortcutRecordingResult {
                        action: recording.action,
                        binding: Some(ShortcutBinding::double_left_alt()),
                        cancelled: false,
                        error: None,
                    }));
            } else if regular_key_down {
                recording_finished = true;
                match recorded_chord {
                    Some(accelerator) => {
                        let _ =
                            engine
                                .sender
                                .send(EngineEvent::Recorded(ShortcutRecordingResult {
                                    action: recording.action,
                                    binding: Some(ShortcutBinding::chord(accelerator)),
                                    cancelled: false,
                                    error: None,
                                }));
                    }
                    None => {
                        let _ =
                            engine
                                .sender
                                .send(EngineEvent::Recorded(ShortcutRecordingResult {
                                    action: recording.action,
                                    binding: None,
                                    cancelled: false,
                                    error: Some(
                                        "该按键暂不支持作为 ReadRay 全局快捷键。".to_string(),
                                    ),
                                }));
                    }
                }
            }
            if recording_finished {
                state.recording = None;
                state.pressed.clear();
            }
            return true;
        }

        if recording_only {
            return false;
        }

        if let Some(action) = state.runtime_action {
            if let Some(phase) = state
                .runtime_tracker
                .update(virtual_key, key_down, repeat, now)
            {
                let _ = engine.sender.send(EngineEvent::Trigger(action, phase));
            }
        }
        false
    }

    fn is_modifier(virtual_key: u32) -> bool {
        matches!(
            virtual_key,
            VK_LMENU
                | VK_RMENU
                | VK_LCONTROL
                | VK_RCONTROL
                | VK_LSHIFT
                | VK_RSHIFT
                | VK_LWIN
                | VK_RWIN
        )
    }

    fn chord_accelerator(pressed: &HashSet<u32>, key: u32) -> Option<String> {
        let mut parts = Vec::new();
        if pressed.contains(&VK_LCONTROL) || pressed.contains(&VK_RCONTROL) {
            parts.push("Ctrl".to_string());
        }
        if pressed.contains(&VK_LMENU) || pressed.contains(&VK_RMENU) {
            parts.push("Alt".to_string());
        }
        if pressed.contains(&VK_LSHIFT) || pressed.contains(&VK_RSHIFT) {
            parts.push("Shift".to_string());
        }
        if pressed.contains(&VK_LWIN) || pressed.contains(&VK_RWIN) {
            parts.push("Super".to_string());
        }
        if parts.is_empty() {
            return None;
        }
        parts.push(key_name(key)?);
        Some(parts.join("+"))
    }

    fn key_name(key: u32) -> Option<String> {
        match key {
            0x41..=0x5A => char::from_u32(key).map(|value| value.to_string()),
            0x30..=0x39 => char::from_u32(key).map(|value| value.to_string()),
            0x70..=0x87 => Some(format!("F{}", key - 0x6F)),
            0x20 => Some("Space".to_string()),
            0x0D => Some("Enter".to_string()),
            0x09 => Some("Tab".to_string()),
            0x08 => Some("Backspace".to_string()),
            0x2E => Some("Delete".to_string()),
            0x2D => Some("Insert".to_string()),
            0x24 => Some("Home".to_string()),
            0x23 => Some("End".to_string()),
            0x21 => Some("PageUp".to_string()),
            0x22 => Some("PageDown".to_string()),
            0x26 => Some("ArrowUp".to_string()),
            0x28 => Some("ArrowDown".to_string()),
            0x25 => Some("ArrowLeft".to_string()),
            0x27 => Some("ArrowRight".to_string()),
            _ => None,
        }
    }

    fn web_key_code(code: &str) -> u32 {
        if let Some(letter) = code.strip_prefix("Key") {
            if letter.len() == 1 {
                let value = letter.as_bytes()[0];
                if value.is_ascii_uppercase() {
                    return u32::from(value);
                }
            }
        }
        if let Some(digit) = code.strip_prefix("Digit") {
            if digit.len() == 1 {
                let value = digit.as_bytes()[0];
                if value.is_ascii_digit() {
                    return u32::from(value);
                }
            }
        }
        if let Some(function) = code.strip_prefix('F') {
            if let Ok(number) = function.parse::<u32>() {
                if (1..=24).contains(&number) {
                    return 0x6F + number;
                }
            }
        }
        match code {
            "Escape" => VK_ESCAPE,
            "AltLeft" => VK_LMENU,
            "AltRight" => VK_RMENU,
            "ControlLeft" => VK_LCONTROL,
            "ControlRight" => VK_RCONTROL,
            "ShiftLeft" => VK_LSHIFT,
            "ShiftRight" => VK_RSHIFT,
            "MetaLeft" => VK_LWIN,
            "MetaRight" => VK_RWIN,
            "Space" => 0x20,
            "Enter" | "NumpadEnter" => 0x0D,
            "Tab" => 0x09,
            "Backspace" => 0x08,
            "Delete" => 0x2E,
            "Insert" => 0x2D,
            "Home" => 0x24,
            "End" => 0x23,
            "PageUp" => 0x21,
            "PageDown" => 0x22,
            "ArrowUp" => 0x26,
            "ArrowDown" => 0x28,
            "ArrowLeft" => 0x25,
            "ArrowRight" => 0x27,
            _ => 0,
        }
    }

    pub(super) fn configure(app: &AppHandle, action: Option<ShortcutAction>) -> Result<(), String> {
        if action.is_some() {
            ensure_started(app)?;
        }
        let engine = engine(app);
        let mut state = engine.state.lock().map_err(|error| error.to_string())?;
        state.runtime_action = action;
        state.runtime_tracker.reset();
        Ok(())
    }

    pub(super) fn begin_recording(
        app: &AppHandle,
        action: ShortcutRecordingAction,
    ) -> Result<(), String> {
        ensure_started(app)?;
        clear_system_menu_suppression();
        let engine = engine(app);
        let mut state = engine.state.lock().map_err(|error| error.to_string())?;
        state.recording = Some(RecordingState {
            action,
            tracker: DoubleTapTracker::default(),
        });
        state.pressed.clear();
        Ok(())
    }

    pub(super) fn submit_recording_key_event(
        app: &AppHandle,
        code: &str,
        key_down: bool,
    ) -> Result<(), String> {
        let engine = engine(app);
        process_input(engine, web_key_code(code), key_down, true);
        Ok(())
    }

    pub(super) fn cancel_recording(_app: &AppHandle) -> Result<(), String> {
        let Some(engine) = ENGINE.get() else {
            return Ok(());
        };
        let mut state = engine.state.lock().map_err(|error| error.to_string())?;
        if let Some(recording) = state.recording.take() {
            let _ = engine
                .sender
                .send(EngineEvent::Recorded(ShortcutRecordingResult {
                    action: recording.action,
                    binding: None,
                    cancelled: true,
                    error: None,
                }));
        }
        state.pressed.clear();
        clear_system_menu_suppression();
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn chord_names_keep_tauri_accelerator_order() {
            let pressed = [VK_LMENU, VK_LWIN, 0x20].into_iter().collect();
            assert_eq!(
                chord_accelerator(&pressed, 0x20).as_deref(),
                Some("Alt+Super+Space")
            );
        }

        #[test]
        fn web_key_codes_match_windows_virtual_keys() {
            assert_eq!(web_key_code("ControlLeft"), VK_LCONTROL);
            assert_eq!(web_key_code("KeyK"), 0x4B);
            assert_eq!(web_key_code("Digit7"), 0x37);
            assert_eq!(web_key_code("F24"), 0x87);
            assert_eq!(web_key_code("NumpadEnter"), 0x0D);
            assert_eq!(web_key_code("Unidentified"), 0);
        }

        #[test]
        fn web_key_sequence_uses_native_recorder_and_clears_completed_state() {
            let (sender, receiver) = mpsc::channel();
            let engine = Engine {
                state: Mutex::new(HookState {
                    recording: Some(RecordingState {
                        action: ShortcutRecordingAction::QuickQuery,
                        tracker: DoubleTapTracker::default(),
                    }),
                    ..HookState::default()
                }),
                sender,
                installed: AtomicBool::new(true),
                start_lock: Mutex::new(()),
            };

            assert!(process_input(
                &engine,
                web_key_code("ControlLeft"),
                true,
                true,
            ));
            assert!(process_input(
                &engine,
                web_key_code("ShiftLeft"),
                true,
                true,
            ));
            assert!(process_input(&engine, web_key_code("KeyK"), true, true,));

            match receiver.recv_timeout(Duration::from_millis(50)).unwrap() {
                EngineEvent::Recorded(result) => {
                    assert_eq!(result.action, ShortcutRecordingAction::QuickQuery);
                    assert_eq!(result.binding, Some(ShortcutBinding::chord("Ctrl+Shift+K")));
                    assert!(!result.cancelled);
                    assert_eq!(result.error, None);
                }
                EngineEvent::Trigger(_, _) => panic!("录制事件不应触发运行期快捷键"),
            }

            let state = engine.state.lock().unwrap();
            assert!(state.recording.is_none());
            assert!(state.pressed.is_empty());
        }

        #[test]
        fn system_accelerator_sequence_records_alt_super_space() {
            let (sender, receiver) = mpsc::channel();
            let engine = Engine {
                state: Mutex::new(HookState {
                    recording: Some(RecordingState {
                        action: ShortcutRecordingAction::SelectionExplanation,
                        tracker: DoubleTapTracker::default(),
                    }),
                    ..HookState::default()
                }),
                sender,
                installed: AtomicBool::new(true),
                start_lock: Mutex::new(()),
            };

            assert!(process_input(&engine, VK_LMENU, true, true));
            assert!(process_input(&engine, VK_LWIN, true, true));
            assert!(process_input(&engine, 0x20, true, true));

            match receiver.recv_timeout(Duration::from_millis(50)).unwrap() {
                EngineEvent::Recorded(result) => {
                    assert_eq!(result.action, ShortcutRecordingAction::SelectionExplanation);
                    assert_eq!(
                        result.binding,
                        Some(ShortcutBinding::chord("Alt+Super+Space"))
                    );
                    assert!(!result.cancelled);
                    assert_eq!(result.error, None);
                }
                EngineEvent::Trigger(_, _) => panic!("录制事件不应触发运行期快捷键"),
            }
        }

        #[test]
        fn webview_system_key_kinds_are_mapped_to_press_and_release() {
            assert_eq!(
                webview_key_down(COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN),
                Some(true)
            );
            assert_eq!(
                webview_key_down(COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP),
                Some(false)
            );
        }

        #[test]
        fn webview_generic_modifiers_are_normalized_to_the_physical_side() {
            assert_eq!(normalize_webview_virtual_key(VK_CONTROL, 0), VK_LCONTROL);
            assert_eq!(
                normalize_webview_virtual_key(VK_CONTROL, 1 << 24),
                VK_RCONTROL
            );
            assert_eq!(normalize_webview_virtual_key(VK_MENU, 0), VK_LMENU);
            assert_eq!(normalize_webview_virtual_key(VK_MENU, 1 << 24), VK_RMENU);
            assert_eq!(
                normalize_webview_virtual_key(VK_SHIFT, 0x2A << 16),
                VK_LSHIFT
            );
            assert_eq!(
                normalize_webview_virtual_key(VK_SHIFT, 0x36 << 16),
                VK_RSHIFT
            );
            assert_eq!(normalize_webview_virtual_key(0x20, 0), 0x20);
        }

        #[test]
        fn webview_modifier_press_does_not_finish_recording_as_unsupported() {
            let (sender, receiver) = mpsc::channel();
            let engine = Engine {
                state: Mutex::new(HookState {
                    recording: Some(RecordingState {
                        action: ShortcutRecordingAction::QuickQuery,
                        tracker: DoubleTapTracker::default(),
                    }),
                    ..HookState::default()
                }),
                sender,
                installed: AtomicBool::new(true),
                start_lock: Mutex::new(()),
            };

            let control = normalize_webview_virtual_key(VK_CONTROL, 0);
            assert!(process_input(&engine, control, true, true));
            assert!(receiver.try_recv().is_err());

            let state = engine.state.lock().unwrap();
            assert!(state.recording.is_some());
            assert!(state.pressed.contains(&VK_LCONTROL));
        }

        #[test]
        fn only_sc_keymenu_is_recognized_as_the_system_menu_command() {
            assert!(is_system_menu_command(
                WM_SYSCOMMAND,
                WPARAM(SC_KEYMENU as usize)
            ));
            assert!(is_system_menu_command(
                WM_SYSCOMMAND,
                WPARAM((SC_KEYMENU | 0x000F) as usize)
            ));
            assert!(!is_system_menu_command(WM_KEYDOWN, WPARAM(0)));
            assert!(!is_system_menu_command(WM_SYSCOMMAND, WPARAM(0xF060)));
        }

        #[test]
        fn double_tap_requires_two_complete_left_alt_taps() {
            let start = Instant::now();
            let mut tracker = DoubleTapTracker::default();
            assert_eq!(tracker.update(VK_LMENU, true, false, start), None);
            assert_eq!(tracker.update(VK_LMENU, false, false, start), None);
            assert_eq!(
                tracker.update(VK_LMENU, true, false, start + Duration::from_millis(200)),
                Some(ShortcutPhase::Pressed)
            );
            assert_eq!(
                tracker.update(VK_LMENU, false, false, start + Duration::from_millis(220)),
                Some(ShortcutPhase::Released)
            );
        }

        #[test]
        fn double_tap_is_cancelled_by_timeout_or_any_intervening_key() {
            let start = Instant::now();
            let mut tracker = DoubleTapTracker::default();
            assert_eq!(tracker.update(VK_LMENU, true, false, start), None);
            assert_eq!(tracker.update(VK_LMENU, false, false, start), None);
            assert_eq!(
                tracker.update(VK_LMENU, true, false, start + Duration::from_millis(351)),
                None
            );
            assert_eq!(
                tracker.update(VK_LMENU, false, false, start + Duration::from_millis(360)),
                None
            );
            assert_eq!(
                tracker.update(VK_LCONTROL, true, false, start + Duration::from_millis(370)),
                None
            );
            assert_eq!(
                tracker.update(VK_LMENU, true, false, start + Duration::from_millis(380)),
                None
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub(super) fn configure(
        _app: &AppHandle,
        action: Option<ShortcutAction>,
    ) -> Result<(), String> {
        if action.is_some() {
            Err("双击修饰键快捷键目前只支持 Windows。".to_string())
        } else {
            Ok(())
        }
    }

    pub(super) fn begin_recording(
        _app: &AppHandle,
        _action: ShortcutRecordingAction,
    ) -> Result<(), String> {
        Err("原生快捷键录制目前只支持 Windows。".to_string())
    }

    pub(super) fn cancel_recording(_app: &AppHandle) -> Result<(), String> {
        Ok(())
    }

    pub(super) fn submit_recording_key_event(
        _app: &AppHandle,
        _code: &str,
        _key_down: bool,
    ) -> Result<(), String> {
        Err("原生快捷键录制目前只支持 Windows。".to_string())
    }

    pub(super) fn install_webview_accelerator_recording(
        _app: &AppHandle,
        _webview_label: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub(crate) fn install_webview_accelerator_recording(
    app: &AppHandle,
    webview_label: &str,
) -> Result<(), String> {
    platform::install_webview_accelerator_recording(app, webview_label)
}

pub(crate) fn configure_double_left_alt(
    app: &AppHandle,
    action: Option<ShortcutAction>,
) -> Result<(), String> {
    platform::configure(app, action)
}

#[tauri::command]
pub fn begin_shortcut_recording(
    app: AppHandle,
    action: ShortcutRecordingAction,
) -> Result<(), String> {
    platform::begin_recording(&app, action)
}

#[tauri::command]
pub fn submit_shortcut_recording_key_event(
    app: AppHandle,
    code: String,
    key_down: bool,
) -> Result<(), String> {
    platform::submit_recording_key_event(&app, &code, key_down)
}

#[tauri::command]
pub fn cancel_shortcut_recording(app: AppHandle) -> Result<(), String> {
    platform::cancel_recording(&app)
}
