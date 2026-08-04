use crate::settings::{AppPreferences, CloseBehavior};
use serde::Serialize;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub const DEFAULT_QUICK_QUERY_SHORTCUT: &str = "Ctrl+Alt+R";
pub const DEFAULT_SELECTION_EXPLANATION_SHORTCUT: &str = "Ctrl+Alt+U";
pub const AUTOSTART_ARGUMENT: &str = "--readray-autostart";

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_OPEN_ID: &str = "readray-tray-open";
const TRAY_QUICK_QUERY_ID: &str = "readray-tray-quick-query";
const TRAY_EXIT_ID: &str = "readray-tray-exit";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutAction {
    QuickQuery,
    SelectionExplanation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MainCloseAction {
    HideToTray,
    SafeExit,
}

fn main_close_action(behavior: CloseBehavior) -> MainCloseAction {
    match behavior {
        CloseBehavior::HideToTray => MainCloseAction::HideToTray,
        CloseBehavior::Exit => MainCloseAction::SafeExit,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShortcutPair {
    quick_query: Shortcut,
    selection_explanation: Shortcut,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ShortcutRegistrationErrors {
    quick_query: Option<String>,
    selection_explanation: Option<String>,
}

impl ShortcutRegistrationErrors {
    fn summary(&self) -> Option<String> {
        let details = [
            self.quick_query.as_deref(),
            self.selection_explanation.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        (!details.is_empty()).then(|| {
            format!(
                "全局快捷键注册失败，但 ReadRay 已继续启动。请在设置中逐项更换冲突组合：{}",
                details.join("；")
            )
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimePreferences {
    shortcuts: ShortcutPair,
    close_behavior: CloseBehavior,
    registered_shortcuts: HashSet<Shortcut>,
    shortcut_registration_errors: ShortcutRegistrationErrors,
}

#[derive(Clone)]
pub(crate) struct StagedRuntimePreferences {
    old: RuntimePreferences,
    new: RuntimePreferences,
    registered_new: Vec<Shortcut>,
    unregistered_old: Vec<Shortcut>,
}

#[derive(Default)]
struct ExitState {
    next_request_id: u64,
    pending_request_id: Option<u64>,
    exiting: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SafeExitRequest {
    request_id: u64,
}

fn runtime_preferences() -> &'static Mutex<Option<RuntimePreferences>> {
    static STATE: OnceLock<Mutex<Option<RuntimePreferences>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

fn exit_state() -> &'static Mutex<ExitState> {
    static STATE: OnceLock<Mutex<ExitState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(ExitState::default()))
}

fn tauri_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn parse_shortcut(value: &str, label: &str) -> Result<Shortcut, String> {
    let trimmed = value.trim();
    let shortcut =
        Shortcut::from_str(trimmed).map_err(|error| format!("{label}快捷键格式无效：{error}"))?;
    if shortcut.mods.is_empty() {
        return Err(format!("{label}快捷键不能使用裸按键。"));
    }
    Ok(shortcut)
}

fn parse_shortcut_pair(quick_query: &str, selection: &str) -> Result<ShortcutPair, String> {
    let quick_query = parse_shortcut(quick_query, "快速查询")?;
    let selection_explanation = parse_shortcut(selection, "选区解释")?;
    if quick_query == selection_explanation {
        return Err("快速查询和选区解释不能使用同一个快捷键。".to_string());
    }
    Ok(ShortcutPair {
        quick_query,
        selection_explanation,
    })
}

fn runtime_from(preferences: &AppPreferences) -> Result<RuntimePreferences, String> {
    Ok(RuntimePreferences {
        shortcuts: parse_shortcut_pair(
            &preferences.quick_query_shortcut,
            &preferences.selection_explanation_shortcut,
        )?,
        close_behavior: preferences.close_behavior,
        registered_shortcuts: HashSet::new(),
        shortcut_registration_errors: ShortcutRegistrationErrors::default(),
    })
}

pub(crate) fn validate_shortcut_pair(quick_query: &str, selection: &str) -> Result<(), String> {
    parse_shortcut_pair(quick_query, selection).map(|_| ())
}

trait ShortcutRegistrar {
    fn register(&self, shortcut: Shortcut) -> Result<(), String>;
    fn unregister(&self, shortcut: Shortcut) -> Result<(), String>;
}

struct TauriShortcutRegistrar<'a>(&'a AppHandle);

impl ShortcutRegistrar for TauriShortcutRegistrar<'_> {
    fn register(&self, shortcut: Shortcut) -> Result<(), String> {
        self.0
            .global_shortcut()
            .register(shortcut)
            .map_err(tauri_error)
    }

    fn unregister(&self, shortcut: Shortcut) -> Result<(), String> {
        self.0
            .global_shortcut()
            .unregister(shortcut)
            .map_err(tauri_error)
    }
}

fn pair_set(pair: ShortcutPair) -> HashSet<Shortcut> {
    [pair.quick_query, pair.selection_explanation]
        .into_iter()
        .collect()
}

fn rollback_registration<R: ShortcutRegistrar>(
    registrar: &R,
    registered_new: &[Shortcut],
    unregistered_old: &[Shortcut],
) -> Result<(), String> {
    let mut errors = Vec::new();
    for shortcut in unregistered_old.iter().copied() {
        if let Err(error) = registrar.register(shortcut) {
            errors.push(format!("恢复 {} 失败：{error}", shortcut));
        }
    }
    for shortcut in registered_new.iter().copied() {
        if let Err(error) = registrar.unregister(shortcut) {
            errors.push(format!("撤销 {} 失败：{error}", shortcut));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

#[cfg(test)]
fn stage_registration<R: ShortcutRegistrar>(
    registrar: &R,
    old: ShortcutPair,
    new: ShortcutPair,
) -> Result<(Vec<Shortcut>, Vec<Shortcut>), String> {
    stage_registration_from_active(registrar, &pair_set(old), &pair_set(new))
}

fn stage_registration_from_active<R: ShortcutRegistrar>(
    registrar: &R,
    active_set: &HashSet<Shortcut>,
    new_set: &HashSet<Shortcut>,
) -> Result<(Vec<Shortcut>, Vec<Shortcut>), String> {
    let mut registered_new = Vec::new();
    let mut unregistered_old = Vec::new();

    for shortcut in new_set.difference(active_set).copied() {
        if let Err(error) = registrar.register(shortcut) {
            let rollback = rollback_registration(registrar, &registered_new, &[]);
            return Err(format!(
                "快捷键 {} 无法注册：{error}{}",
                shortcut,
                rollback
                    .err()
                    .map(|detail| format!("；运行时回滚失败：{detail}"))
                    .unwrap_or_default()
            ));
        }
        registered_new.push(shortcut);
    }

    for shortcut in active_set.difference(new_set).copied() {
        if let Err(error) = registrar.unregister(shortcut) {
            let rollback = rollback_registration(registrar, &registered_new, &unregistered_old);
            return Err(format!(
                "旧快捷键 {} 无法注销：{error}{}",
                shortcut,
                rollback
                    .err()
                    .map(|detail| format!("；运行时回滚失败：{detail}"))
                    .unwrap_or_default()
            ));
        }
        unregistered_old.push(shortcut);
    }

    Ok((registered_new, unregistered_old))
}

fn register_startup_shortcuts<R: ShortcutRegistrar>(
    registrar: &R,
    shortcuts: ShortcutPair,
) -> (HashSet<Shortcut>, ShortcutRegistrationErrors) {
    let mut registered = HashSet::new();
    let mut errors = ShortcutRegistrationErrors::default();
    match registrar.register(shortcuts.quick_query) {
        Ok(()) => {
            registered.insert(shortcuts.quick_query);
        }
        Err(error) => {
            errors.quick_query = Some(format!(
                "快速查询快捷键 {} 无法注册：{error}",
                shortcuts.quick_query
            ));
        }
    }
    match registrar.register(shortcuts.selection_explanation) {
        Ok(()) => {
            registered.insert(shortcuts.selection_explanation);
        }
        Err(error) => {
            errors.selection_explanation = Some(format!(
                "选区解释快捷键 {} 无法注册：{error}",
                shortcuts.selection_explanation
            ));
        }
    }
    (registered, errors)
}

fn stage_runtime_registration<R: ShortcutRegistrar>(
    registrar: &R,
    active: RuntimePreferences,
    mut candidate: RuntimePreferences,
) -> Result<StagedRuntimePreferences, String> {
    let quick_changed = active.shortcuts.quick_query != candidate.shortcuts.quick_query;
    let selection_changed =
        active.shortcuts.selection_explanation != candidate.shortcuts.selection_explanation;

    let mut target_registered = active.registered_shortcuts.clone();
    if quick_changed {
        target_registered.insert(candidate.shortcuts.quick_query);
    }
    if selection_changed {
        target_registered.insert(candidate.shortcuts.selection_explanation);
    }

    let candidate_pair = pair_set(candidate.shortcuts);
    if quick_changed && !candidate_pair.contains(&active.shortcuts.quick_query) {
        target_registered.remove(&active.shortcuts.quick_query);
    }
    if selection_changed && !candidate_pair.contains(&active.shortcuts.selection_explanation) {
        target_registered.remove(&active.shortcuts.selection_explanation);
    }

    let (registered_new, unregistered_old) = stage_registration_from_active(
        registrar,
        &active.registered_shortcuts,
        &target_registered,
    )?;
    candidate.registered_shortcuts = target_registered;
    candidate.shortcut_registration_errors = active.shortcut_registration_errors.clone();
    if quick_changed {
        candidate.shortcut_registration_errors.quick_query = None;
    }
    if selection_changed {
        candidate.shortcut_registration_errors.selection_explanation = None;
    }

    Ok(StagedRuntimePreferences {
        old: active,
        new: candidate,
        registered_new,
        unregistered_old,
    })
}

pub(crate) fn initialize_runtime_preferences(
    app: &AppHandle,
    preferences: &AppPreferences,
) -> Result<(), String> {
    let mut runtime = runtime_from(preferences)?;
    let registrar = TauriShortcutRegistrar(app);
    let (registered, errors) = register_startup_shortcuts(&registrar, runtime.shortcuts);
    if let Some(detail) = errors.summary() {
        eprintln!("READRAY_SHORTCUT_REGISTRATION_ERROR={detail}");
    }
    runtime.registered_shortcuts = registered;
    runtime.shortcut_registration_errors = errors;
    *runtime_preferences().lock().map_err(tauri_error)? = Some(runtime);
    Ok(())
}

pub(crate) fn shortcut_registration_error() -> Option<String> {
    runtime_preferences()
        .lock()
        .ok()
        .and_then(|runtime| runtime.as_ref()?.shortcut_registration_errors.summary())
}

pub(crate) fn stage_runtime_preferences(
    app: &AppHandle,
    current: &AppPreferences,
    candidate: &AppPreferences,
) -> Result<StagedRuntimePreferences, String> {
    let current_runtime = runtime_from(current)?;
    let candidate_runtime = runtime_from(candidate)?;
    let active = runtime_preferences()
        .lock()
        .map_err(tauri_error)?
        .clone()
        .ok_or_else(|| "全局快捷键运行时尚未初始化。".to_string())?;
    if active.shortcuts.quick_query != current_runtime.shortcuts.quick_query
        || active.shortcuts.selection_explanation != current_runtime.shortcuts.selection_explanation
        || active.close_behavior != current_runtime.close_behavior
    {
        return Err("运行时设置与 SQLite 权威值不一致，请重启 ReadRay 后重试。".to_string());
    }
    stage_runtime_registration(&TauriShortcutRegistrar(app), active, candidate_runtime)
}

pub(crate) fn rollback_runtime_preferences(
    app: &AppHandle,
    staged: StagedRuntimePreferences,
    original_error: String,
) -> String {
    let rollback = rollback_registration(
        &TauriShortcutRegistrar(app),
        &staged.registered_new,
        &staged.unregistered_old,
    );
    *runtime_preferences()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(staged.old);
    match rollback {
        Ok(()) => original_error,
        Err(detail) => format!("{original_error}；全局快捷键回滚失败：{detail}"),
    }
}

pub(crate) fn commit_runtime_preferences(
    staged: StagedRuntimePreferences,
    saved: &AppPreferences,
) -> Result<(), String> {
    let mut saved_runtime = runtime_from(saved)?;
    if saved_runtime.shortcuts.quick_query != staged.new.shortcuts.quick_query
        || saved_runtime.shortcuts.selection_explanation
            != staged.new.shortcuts.selection_explanation
        || saved_runtime.close_behavior != staged.new.close_behavior
    {
        return Err("SQLite 提交结果与已注册的运行时设置不一致。".to_string());
    }
    saved_runtime.registered_shortcuts = staged.new.registered_shortcuts;
    saved_runtime.shortcut_registration_errors = staged.new.shortcut_registration_errors;
    *runtime_preferences().lock().map_err(tauri_error)? = Some(saved_runtime);
    Ok(())
}

pub(crate) fn shortcut_action(shortcut: &Shortcut) -> Option<ShortcutAction> {
    let guard = runtime_preferences().lock().ok()?;
    let runtime = guard.as_ref()?;
    shortcut_action_for(runtime, shortcut)
}

fn shortcut_action_for(
    runtime: &RuntimePreferences,
    shortcut: &Shortcut,
) -> Option<ShortcutAction> {
    if !runtime.registered_shortcuts.contains(shortcut) {
        return None;
    }
    if shortcut == &runtime.shortcuts.quick_query {
        Some(ShortcutAction::QuickQuery)
    } else if shortcut == &runtime.shortcuts.selection_explanation {
        Some(ShortcutAction::SelectionExplanation)
    } else {
        None
    }
}

pub(crate) fn close_behavior() -> CloseBehavior {
    runtime_preferences()
        .lock()
        .ok()
        .and_then(|runtime| runtime.as_ref().map(|runtime| runtime.close_behavior))
        .unwrap_or(CloseBehavior::HideToTray)
}

pub(crate) fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| "ReadRay 主窗口不存在。".to_string())?;
    if window.is_minimized().map_err(tauri_error)? {
        window.unminimize().map_err(tauri_error)?;
    }
    window.show().map_err(tauri_error)?;
    window.set_focus().map_err(tauri_error)
}

pub(crate) fn launched_from_autostart() -> bool {
    std::env::args().any(|argument| argument == AUTOSTART_ARGUMENT)
}

pub(crate) fn setup_tray(app: &tauri::App) -> Result<(), String> {
    let menu = MenuBuilder::new(app)
        .text(TRAY_OPEN_ID, "打开 ReadRay")
        .text(TRAY_QUICK_QUERY_ID, "快速查询")
        .text(TRAY_EXIT_ID, "退出 ReadRay")
        .build()
        .map_err(tauri_error)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "ReadRay 托盘图标资源缺失。".to_string())?;

    TrayIconBuilder::with_id("readray-main-tray")
        .icon(icon)
        .tooltip("ReadRay")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN_ID => {
                if let Err(error) = show_main_window(app) {
                    eprintln!("READRAY_TRAY_OPEN_ERROR={error}");
                }
            }
            TRAY_QUICK_QUERY_ID => {
                if let Err(error) = crate::wake_quick_query(app) {
                    eprintln!("READRAY_TRAY_QUICK_QUERY_ERROR={error}");
                }
            }
            TRAY_EXIT_ID => {
                if let Err(error) = request_safe_exit(app) {
                    eprintln!("READRAY_TRAY_EXIT_ERROR={error}");
                }
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                if let Err(error) = show_main_window(tray.app_handle()) {
                    eprintln!("READRAY_TRAY_LEFT_CLICK_ERROR={error}");
                }
            }
        })
        .build(app)
        .map_err(tauri_error)?;
    Ok(())
}

pub(crate) fn handle_main_close(app: &AppHandle, window: &tauri::Window) {
    match main_close_action(close_behavior()) {
        MainCloseAction::HideToTray => {
            let _ = window.hide();
        }
        MainCloseAction::SafeExit => {
            if let Err(error) = request_safe_exit(app) {
                eprintln!("READRAY_CLOSE_EXIT_ERROR={error}");
                let _ = show_main_window(app);
            }
        }
    }
}

pub(crate) fn request_safe_exit(app: &AppHandle) -> Result<u64, String> {
    let request_id = {
        let mut state = exit_state().lock().map_err(tauri_error)?;
        begin_exit_request(&mut state)?
    };

    if let Err(error) = app.emit_to(
        MAIN_WINDOW_LABEL,
        "readray://safe-exit-requested",
        SafeExitRequest { request_id },
    ) {
        if let Ok(mut state) = exit_state().lock() {
            if state.pending_request_id == Some(request_id) {
                state.pending_request_id = None;
            }
        }
        let _ = show_main_window(app);
        return Err(format!("无法通知主窗口保存数据：{error}"));
    }
    Ok(request_id)
}

fn begin_exit_request(state: &mut ExitState) -> Result<u64, String> {
    if state.exiting {
        return Err("ReadRay 已进入退出流程。".to_string());
    }
    if let Some(request_id) = state.pending_request_id {
        return Ok(request_id);
    }
    state.next_request_id = state.next_request_id.saturating_add(1).max(1);
    let request_id = state.next_request_id;
    state.pending_request_id = Some(request_id);
    Ok(request_id)
}

fn claim_exit_request_state(state: &mut ExitState, request_id: u64) -> Result<(), String> {
    if state.pending_request_id != Some(request_id) {
        return Err("退出请求已过期。".to_string());
    }
    state.pending_request_id = None;
    state.exiting = true;
    Ok(())
}

fn claim_exit_request(request_id: u64) -> Result<(), String> {
    let mut state = exit_state().lock().map_err(tauri_error)?;
    claim_exit_request_state(&mut state, request_id)
}

fn cancel_exit_request(state: &mut ExitState, request_id: u64) -> Result<(), String> {
    if state.exiting {
        return Err("退出请求已过期。".to_string());
    }
    match state.pending_request_id {
        Some(pending_request_id) if pending_request_id == request_id => {
            state.pending_request_id = None;
            Ok(())
        }
        None if request_id <= state.next_request_id => Ok(()),
        _ => Err("退出请求已过期。".to_string()),
    }
}

fn cancel_exit_with_restore(
    state: &Mutex<ExitState>,
    request_id: u64,
    restore: impl FnOnce() -> Result<(), String>,
) -> Result<Option<String>, String> {
    {
        let mut state = state.lock().map_err(tauri_error)?;
        cancel_exit_request(&mut state, request_id)?;
    }
    Ok(restore().err())
}

#[tauri::command]
pub fn request_app_exit(app: AppHandle) -> Result<u64, String> {
    request_safe_exit(&app)
}

#[tauri::command]
pub fn get_pending_app_exit_request() -> Result<Option<u64>, String> {
    Ok(exit_state().lock().map_err(tauri_error)?.pending_request_id)
}

#[tauri::command]
pub fn restore_main_window(app: AppHandle) -> Result<(), String> {
    show_main_window(&app)
}

#[tauri::command]
pub fn cancel_app_exit(app: AppHandle, request_id: u64) -> Result<(), String> {
    let restore_error =
        cancel_exit_with_restore(exit_state(), request_id, || show_main_window(&app))?;
    if let Some(error) = restore_error {
        eprintln!("READRAY_CANCEL_EXIT_RESTORE_WARNING={error}");
    }
    Ok(())
}

#[tauri::command]
pub fn complete_app_exit(app: AppHandle, request_id: u64) -> Result<(), String> {
    claim_exit_request(request_id)?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn force_app_exit(app: AppHandle, request_id: u64) -> Result<(), String> {
    claim_exit_request(request_id)?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn apply_main_window_close_behavior(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<(), String> {
    if window.label() != MAIN_WINDOW_LABEL {
        return Err("该命令只允许主窗口调用。".to_string());
    }
    match main_close_action(close_behavior()) {
        MainCloseAction::HideToTray => window.hide().map_err(tauri_error)?,
        MainCloseAction::SafeExit => {
            request_safe_exit(&app)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeRegistrar {
        registered: RefCell<HashSet<Shortcut>>,
        fail_register: RefCell<HashSet<Shortcut>>,
        fail_unregister: Option<Shortcut>,
    }

    impl ShortcutRegistrar for FakeRegistrar {
        fn register(&self, shortcut: Shortcut) -> Result<(), String> {
            if self.fail_register.borrow().contains(&shortcut) {
                return Err("模拟注册失败".to_string());
            }
            self.registered.borrow_mut().insert(shortcut);
            Ok(())
        }

        fn unregister(&self, shortcut: Shortcut) -> Result<(), String> {
            if self.fail_unregister == Some(shortcut) {
                return Err("模拟注销失败".to_string());
            }
            self.registered.borrow_mut().remove(&shortcut);
            Ok(())
        }
    }

    fn pair(quick: &str, selection: &str) -> ShortcutPair {
        parse_shortcut_pair(quick, selection).unwrap()
    }

    fn runtime(shortcuts: ShortcutPair) -> RuntimePreferences {
        RuntimePreferences {
            shortcuts,
            close_behavior: CloseBehavior::HideToTray,
            registered_shortcuts: HashSet::new(),
            shortcut_registration_errors: ShortcutRegistrationErrors::default(),
        }
    }

    #[test]
    fn shortcut_validation_rejects_bare_duplicate_and_invalid_values() {
        assert!(validate_shortcut_pair("R", "Ctrl+Alt+U")
            .unwrap_err()
            .contains("裸按键"));
        assert!(validate_shortcut_pair("Ctrl+Alt+R", "Ctrl+Alt+R")
            .unwrap_err()
            .contains("同一个"));
        assert!(validate_shortcut_pair("Ctrl+NoSuchKey", "Ctrl+Alt+U").is_err());
    }

    #[test]
    fn registration_failure_keeps_original_shortcuts() {
        let old = pair("Ctrl+Alt+R", "Ctrl+Alt+U");
        let new = pair("Ctrl+Shift+R", "Ctrl+Alt+U");
        let registrar = FakeRegistrar {
            registered: RefCell::new(pair_set(old)),
            fail_register: RefCell::new([new.quick_query].into_iter().collect()),
            fail_unregister: None,
        };
        assert!(stage_registration(&registrar, old, new).is_err());
        assert_eq!(*registrar.registered.borrow(), pair_set(old));
    }

    #[test]
    fn startup_shortcut_conflict_degrades_without_aborting_other_runtime_features() {
        let shortcuts = pair("Ctrl+Alt+R", "Ctrl+Alt+U");
        let registrar = FakeRegistrar {
            registered: RefCell::new(HashSet::new()),
            fail_register: RefCell::new([shortcuts.quick_query].into_iter().collect()),
            fail_unregister: None,
        };
        let (registered, errors) = register_startup_shortcuts(&registrar, shortcuts);
        assert!(!registered.contains(&shortcuts.quick_query));
        assert!(registered.contains(&shortcuts.selection_explanation));
        assert!(errors.summary().unwrap().contains("已继续启动"));
    }

    #[test]
    fn database_failure_rollback_restores_original_shortcuts() {
        let old = pair("Ctrl+Alt+R", "Ctrl+Alt+U");
        let new = pair("Ctrl+Shift+R", "Ctrl+Shift+U");
        let registrar = FakeRegistrar {
            registered: RefCell::new(pair_set(old)),
            fail_register: RefCell::new(HashSet::new()),
            fail_unregister: None,
        };
        let (registered_new, unregistered_old) = stage_registration(&registrar, old, new).unwrap();
        assert_eq!(*registrar.registered.borrow(), pair_set(new));
        rollback_registration(&registrar, &registered_new, &unregistered_old).unwrap();
        assert_eq!(*registrar.registered.borrow(), pair_set(old));
    }

    #[test]
    fn database_failure_rollback_preserves_active_metadata_and_responses() {
        let shortcuts = pair("Ctrl+Alt+R", "Ctrl+Alt+U");
        let mut active = runtime(shortcuts);
        active.registered_shortcuts = pair_set(shortcuts);
        active.shortcut_registration_errors.selection_explanation =
            Some("选区解释快捷键 Ctrl+Alt+U 无法注册：occupied".to_string());
        let mut candidate = runtime(shortcuts);
        candidate.close_behavior = CloseBehavior::Exit;
        let registrar = FakeRegistrar {
            registered: RefCell::new(active.registered_shortcuts.clone()),
            fail_register: RefCell::new(HashSet::new()),
            fail_unregister: None,
        };

        let staged = stage_runtime_registration(&registrar, active.clone(), candidate).unwrap();
        assert_eq!(staged.old, active, "回滚元数据必须来自实际 active 状态");
        rollback_registration(&registrar, &staged.registered_new, &staged.unregistered_old)
            .unwrap();
        let restored = staged.old;
        assert_eq!(
            *registrar.registered.borrow(),
            restored.registered_shortcuts
        );
        assert_eq!(
            shortcut_action_for(&restored, &shortcuts.quick_query),
            Some(ShortcutAction::QuickQuery)
        );
        assert_eq!(
            shortcut_action_for(&restored, &shortcuts.selection_explanation),
            Some(ShortcutAction::SelectionExplanation)
        );
        assert!(restored.shortcut_registration_errors.summary().is_some());
    }

    #[test]
    fn two_startup_conflicts_can_be_recovered_one_shortcut_at_a_time() {
        let original = pair("Ctrl+Alt+R", "Ctrl+Alt+U");
        let registrar = FakeRegistrar {
            registered: RefCell::new(HashSet::new()),
            fail_register: RefCell::new(pair_set(original)),
            fail_unregister: None,
        };
        let (registered, errors) = register_startup_shortcuts(&registrar, original);
        assert!(registered.is_empty());
        assert!(errors.quick_query.is_some());
        assert!(errors.selection_explanation.is_some());

        let mut active = runtime(original);
        active.registered_shortcuts = registered;
        active.shortcut_registration_errors = errors;
        let quick_recovered = pair("Ctrl+Shift+R", "Ctrl+Alt+U");
        let first =
            stage_runtime_registration(&registrar, active, runtime(quick_recovered)).unwrap();
        assert_eq!(
            shortcut_action_for(&first.new, &quick_recovered.quick_query),
            Some(ShortcutAction::QuickQuery)
        );
        assert!(first.new.shortcut_registration_errors.quick_query.is_none());
        assert!(first
            .new
            .shortcut_registration_errors
            .selection_explanation
            .is_some());
        let remaining_error = first.new.shortcut_registration_errors.summary().unwrap();
        assert!(remaining_error.contains("选区解释"));
        assert!(!remaining_error.contains("快速查询快捷键"));

        let all_recovered = pair("Ctrl+Shift+R", "Ctrl+Shift+U");
        let second =
            stage_runtime_registration(&registrar, first.new, runtime(all_recovered)).unwrap();
        assert_eq!(*registrar.registered.borrow(), pair_set(all_recovered));
        assert!(second.new.shortcut_registration_errors.summary().is_none());
        assert_eq!(
            shortcut_action_for(&second.new, &all_recovered.quick_query),
            Some(ShortcutAction::QuickQuery)
        );
        assert_eq!(
            shortcut_action_for(&second.new, &all_recovered.selection_explanation),
            Some(ShortcutAction::SelectionExplanation)
        );
    }

    #[test]
    fn close_behavior_defaults_to_hide_before_runtime_initialization() {
        assert_eq!(close_behavior(), CloseBehavior::HideToTray);
        assert_eq!(
            main_close_action(CloseBehavior::HideToTray),
            MainCloseAction::HideToTray
        );
        assert_eq!(
            main_close_action(CloseBehavior::Exit),
            MainCloseAction::SafeExit
        );
    }

    #[test]
    fn cancelling_failed_exit_invalidates_old_request_and_allows_a_new_one() {
        let mut state = ExitState::default();
        let first = begin_exit_request(&mut state).unwrap();
        cancel_exit_request(&mut state, first).unwrap();
        assert_eq!(state.pending_request_id, None);
        assert!(claim_exit_request_state(&mut state, first).is_err());

        let second = begin_exit_request(&mut state).unwrap();
        assert!(second > first);
        assert!(claim_exit_request_state(&mut state, first).is_err());
        claim_exit_request_state(&mut state, second).unwrap();
        assert!(state.exiting);
    }

    #[test]
    fn cancel_exit_stays_successful_when_window_restore_fails() {
        let state = Mutex::new(ExitState::default());
        let request_id = begin_exit_request(&mut state.lock().unwrap()).unwrap();
        let warning =
            cancel_exit_with_restore(
                &state,
                request_id,
                || Err("模拟 set_focus 失败".to_string()),
            )
            .unwrap();
        assert_eq!(state.lock().unwrap().pending_request_id, None);
        assert!(warning.unwrap().contains("set_focus"));
        cancel_exit_with_restore(&state, request_id, || Ok(())).unwrap();
        let mut state = state.lock().unwrap();
        assert!(claim_exit_request_state(&mut state, request_id).is_err());
        assert!(begin_exit_request(&mut state).unwrap() > request_id);
    }
}
