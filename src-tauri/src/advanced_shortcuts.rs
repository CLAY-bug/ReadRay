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
    use tauri::Emitter;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    const DOUBLE_TAP_INTERVAL: Duration = Duration::from_millis(350);
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
        completed: bool,
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
                        if process_input(engine, input.vkCode, key_down) {
                            return LRESULT(1);
                        }
                    }
                }
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    fn process_input(engine: &Engine, virtual_key: u32, key_down: bool) -> bool {
        let now = Instant::now();
        let Ok(mut state) = engine.state.lock() else {
            return false;
        };
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
        if let Some(recording) = state.recording.as_mut() {
            if !recording.completed && key_down && !repeat && virtual_key == VK_ESCAPE {
                recording.completed = true;
                let _ = engine
                    .sender
                    .send(EngineEvent::Recorded(ShortcutRecordingResult {
                        action: recording.action,
                        binding: None,
                        cancelled: true,
                        error: None,
                    }));
            } else if !recording.completed {
                if recording.tracker.update(virtual_key, key_down, repeat, now)
                    == Some(ShortcutPhase::Released)
                {
                    recording.completed = true;
                    let _ = engine
                        .sender
                        .send(EngineEvent::Recorded(ShortcutRecordingResult {
                            action: recording.action,
                            binding: Some(ShortcutBinding::double_left_alt()),
                            cancelled: false,
                            error: None,
                        }));
                } else if regular_key_down {
                    match recorded_chord {
                        Some(accelerator) => {
                            recording.completed = true;
                            let _ = engine.sender.send(EngineEvent::Recorded(
                                ShortcutRecordingResult {
                                    action: recording.action,
                                    binding: Some(ShortcutBinding::chord(accelerator)),
                                    cancelled: false,
                                    error: None,
                                },
                            ));
                        }
                        None => {
                            let _ = engine.sender.send(EngineEvent::Recorded(
                                ShortcutRecordingResult {
                                    action: recording.action,
                                    binding: None,
                                    cancelled: false,
                                    error: Some(
                                        "该按键暂不支持作为 ReadRay 全局快捷键。".to_string(),
                                    ),
                                },
                            ));
                            recording.completed = true;
                        }
                    }
                }
            }
            if recording.completed && state.pressed.is_empty() {
                state.recording = None;
            }
            return true;
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
        let engine = engine(app);
        let mut state = engine.state.lock().map_err(|error| error.to_string())?;
        state.recording = Some(RecordingState {
            action,
            tracker: DoubleTapTracker::default(),
            completed: false,
        });
        state.pressed.clear();
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
pub fn cancel_shortcut_recording(app: AppHandle) -> Result<(), String> {
    platform::cancel_recording(&app)
}
