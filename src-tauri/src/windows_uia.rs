use serde::Serialize;
use std::ffi::c_void;
use std::time::{SystemTime, UNIX_EPOCH};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, POINT, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, SAFEARRAY,
};
use windows::Win32::System::Ole::{
    SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextPattern2, IUIAutomationTextRange, IUIAutomationTreeWalker, TextUnit_Paragraph,
    UIA_TextPattern2Id, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

const MAX_SELECTED_TEXT_CHARS: i32 = 4096;
const MAX_CONTEXT_TEXT_CHARS: i32 = 4096;
const MAX_ANCESTOR_DEPTH: usize = 16;
const MAX_RAW_VIEW_ELEMENTS: usize = 512;
const MAX_RAW_VIEW_DEPTH: usize = 16;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForegroundDiagnostics {
    pub hwnd: usize,
    pub process_id: u32,
    pub executable_path: Option<String>,
    pub window_title: Option<String>,
    pub focused_element_name: Option<String>,
    pub focused_element_class_name: Option<String>,
    pub focused_element_framework_id: Option<String>,
    pub focused_element_control_type: Option<i32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsUiaCapture {
    pub ok: bool,
    pub captured_at_unix_ms: u64,
    pub completed_at_unix_ms: u64,
    pub capture_phase: &'static str,
    pub selected_text: Option<String>,
    pub context_text: Option<String>,
    pub anchor_rect: Option<ScreenRect>,
    pub coordinate_space: &'static str,
    pub foreground: ForegroundDiagnostics,
    pub text_pattern: Option<&'static str>,
    pub text_pattern_source: Option<String>,
    pub selection_range_count: Option<i32>,
    pub bounding_rectangle_count: usize,
    pub uia_candidate_count: usize,
    pub uia_text_pattern_candidate_count: usize,
    pub diagnostics: Vec<String>,
    pub error: Option<String>,
}

impl WindowsUiaCapture {
    fn new() -> Self {
        let captured_at_unix_ms = unix_time_ms();

        Self {
            ok: false,
            captured_at_unix_ms,
            completed_at_unix_ms: captured_at_unix_ms,
            capture_phase: "beforeReadRayShowAndFocus",
            selected_text: None,
            context_text: None,
            anchor_rect: None,
            coordinate_space: "physicalScreenPixels",
            foreground: ForegroundDiagnostics::default(),
            text_pattern: None,
            text_pattern_source: None,
            selection_range_count: None,
            bounding_rectangle_count: 0,
            uia_candidate_count: 0,
            uia_text_pattern_candidate_count: 0,
            diagnostics: Vec::new(),
            error: None,
        }
    }
}

enum TextPatternProvider {
    TextPattern2(IUIAutomationTextPattern2),
    TextPattern(IUIAutomationTextPattern),
}

struct ElementCandidate {
    source: String,
    element: IUIAutomationElement,
}

impl TextPatternProvider {
    unsafe fn selection(
        &self,
    ) -> windows::core::Result<windows::Win32::UI::Accessibility::IUIAutomationTextRangeArray> {
        match self {
            Self::TextPattern2(pattern) => unsafe { pattern.GetSelection() },
            Self::TextPattern(pattern) => unsafe { pattern.GetSelection() },
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::TextPattern2(_) => "TextPattern2",
            Self::TextPattern(_) => "TextPattern",
        }
    }
}

struct ComApartment {
    should_uninitialize: bool,
}

impl ComApartment {
    unsafe fn initialize() -> Result<Self, String> {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_err() && result != RPC_E_CHANGED_MODE {
            return Err(format!("COM 初始化失败：{result:?}"));
        }

        Ok(Self {
            should_uninitialize: result.is_ok(),
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

struct OwnedSafeArray(*mut SAFEARRAY);

impl Drop for OwnedSafeArray {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { SafeArrayDestroy(self.0) };
        }
    }
}

pub fn capture_foreground() -> WindowsUiaCapture {
    let mut capture = WindowsUiaCapture::new();

    if let Err(error) = unsafe { capture_foreground_inner(&mut capture) } {
        capture.error = Some(error);
    } else {
        capture.ok = true;
    }

    capture.completed_at_unix_ms = unix_time_ms();
    capture
}

unsafe fn capture_foreground_inner(capture: &mut WindowsUiaCapture) -> Result<(), String> {
    let foreground_window = unsafe { GetForegroundWindow() };
    if foreground_window.0.is_null() {
        return Err("没有可用的前台窗口。".to_string());
    }

    capture.foreground = foreground_diagnostics(foreground_window);
    let _com = unsafe { ComApartment::initialize()? };
    let automation: IUIAutomation = unsafe {
        CoCreateInstance(
            &CUIAutomation8,
            None::<&windows::core::IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
    }
    .map_err(|error| format!("创建 UI Automation 客户端失败：{error}"))?;
    let focused_element = unsafe { automation.GetFocusedElement() }
        .or_else(|_| unsafe { automation.ElementFromHandle(foreground_window) })
        .map_err(|error| format!("无法取得前台焦点元素：{error}"))?;

    fill_focused_element_diagnostics(&focused_element, &mut capture.foreground);

    let mut candidates = unsafe {
        collect_initial_candidates(
            &automation,
            foreground_window,
            focused_element,
            &mut capture.diagnostics,
        )
    };
    if unsafe { try_candidates(&candidates, capture) } {
        capture.uia_candidate_count = candidates.len();
        return Ok(());
    }
    if capture.uia_text_pattern_candidate_count > 0
        && capture.selection_range_count.unwrap_or_default() > 0
    {
        capture.uia_candidate_count = candidates.len();
        capture
            .diagnostics
            .push("UIA 浅层候选返回退化文本范围，判定当前无文本选区。".to_string());
        return Ok(());
    }

    let initial_candidate_count = candidates.len();
    unsafe {
        collect_raw_view_candidates(
            &automation,
            foreground_window,
            &mut candidates,
            &mut capture.diagnostics,
        )
    };
    if unsafe { try_candidates(&candidates[initial_candidate_count..], capture) } {
        capture.uia_candidate_count = candidates.len();
        return Ok(());
    }

    capture.uia_candidate_count = candidates.len();
    capture.diagnostics.push(format!(
        "已检查 {} 个 UIA 候选，其中 {} 个支持 TextPattern；未取得非退化文本选区。",
        capture.uia_candidate_count, capture.uia_text_pattern_candidate_count
    ));

    Ok(())
}

unsafe fn collect_initial_candidates(
    automation: &IUIAutomation,
    foreground_window: HWND,
    focused_element: IUIAutomationElement,
    diagnostics: &mut Vec<String>,
) -> Vec<ElementCandidate> {
    let mut candidates = Vec::new();
    push_unique_candidate(
        automation,
        &mut candidates,
        "focused".to_string(),
        focused_element.clone(),
    );

    let root = unsafe { automation.ElementFromHandle(foreground_window) }.ok();
    if let Some(root) = root.as_ref() {
        push_unique_candidate(
            automation,
            &mut candidates,
            "foregroundRoot".to_string(),
            root.clone(),
        );
    }

    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_ok() {
        if let Ok(element) = unsafe { automation.ElementFromPoint(cursor) } {
            diagnostics.push(format!(
                "ElementFromPoint({}, {})={}",
                cursor.x,
                cursor.y,
                describe_element(&element)
            ));
            push_unique_candidate(
                automation,
                &mut candidates,
                "cursorPoint".to_string(),
                element,
            );
        }
    }

    let walker = match unsafe { automation.RawViewWalker() } {
        Ok(walker) => walker,
        Err(error) => {
            diagnostics.push(format!("无法取得 UIA Raw View walker：{error}"));
            return candidates;
        }
    };

    let ancestor_seeds: Vec<(String, IUIAutomationElement)> = candidates
        .iter()
        .map(|candidate| (candidate.source.clone(), candidate.element.clone()))
        .collect();
    for (source, element) in ancestor_seeds {
        unsafe { collect_ancestors(automation, &walker, &mut candidates, &source, element) };
    }

    candidates
}

unsafe fn collect_raw_view_candidates(
    automation: &IUIAutomation,
    foreground_window: HWND,
    candidates: &mut Vec<ElementCandidate>,
    diagnostics: &mut Vec<String>,
) {
    let root = match unsafe { automation.ElementFromHandle(foreground_window) } {
        Ok(root) => root,
        Err(error) => {
            diagnostics.push(format!("无法取得 UIA 前台窗口根元素：{error}"));
            return;
        }
    };
    let walker = match unsafe { automation.RawViewWalker() } {
        Ok(walker) => walker,
        Err(error) => {
            diagnostics.push(format!("无法取得 UIA Raw View walker：{error}"));
            return;
        }
    };
    let remaining = MAX_RAW_VIEW_ELEMENTS.saturating_sub(candidates.len());
    unsafe { collect_raw_subtree(automation, &walker, candidates, root, 0, remaining) };
}

unsafe fn try_candidates(candidates: &[ElementCandidate], capture: &mut WindowsUiaCapture) -> bool {
    for candidate in candidates {
        let Some(pattern) = text_pattern_from_element(&candidate.element) else {
            continue;
        };
        capture.uia_text_pattern_candidate_count += 1;

        match unsafe { try_capture_selection(&pattern, candidate, capture) } {
            Ok(true) => return true,
            Ok(false) => {}
            Err(error) => capture.diagnostics.push(error),
        }
    }

    false
}

unsafe fn collect_ancestors(
    automation: &IUIAutomation,
    walker: &IUIAutomationTreeWalker,
    candidates: &mut Vec<ElementCandidate>,
    source: &str,
    mut element: IUIAutomationElement,
) {
    for depth in 1..=MAX_ANCESTOR_DEPTH {
        let Ok(parent) = (unsafe { walker.GetParentElement(&element) }) else {
            break;
        };
        push_unique_candidate(
            automation,
            candidates,
            format!("{source}.ancestor[{depth}]"),
            parent.clone(),
        );
        element = parent;
    }
}

unsafe fn collect_raw_subtree(
    automation: &IUIAutomation,
    walker: &IUIAutomationTreeWalker,
    candidates: &mut Vec<ElementCandidate>,
    parent: IUIAutomationElement,
    depth: usize,
    remaining: usize,
) {
    if depth >= MAX_RAW_VIEW_DEPTH || remaining == 0 {
        return;
    }

    let Ok(mut child) = (unsafe { walker.GetFirstChildElement(&parent) }) else {
        return;
    };
    let mut visited = 0usize;

    loop {
        if candidates.len() >= MAX_RAW_VIEW_ELEMENTS || visited >= remaining {
            break;
        }
        visited += 1;
        let source = format!("rawView.depth[{}]", depth + 1);
        push_unique_candidate(automation, candidates, source, child.clone());
        unsafe {
            collect_raw_subtree(
                automation,
                walker,
                candidates,
                child.clone(),
                depth + 1,
                remaining.saturating_sub(visited),
            )
        };

        let Ok(sibling) = (unsafe { walker.GetNextSiblingElement(&child) }) else {
            break;
        };
        child = sibling;
    }
}

fn push_unique_candidate(
    automation: &IUIAutomation,
    candidates: &mut Vec<ElementCandidate>,
    source: String,
    element: IUIAutomationElement,
) {
    let duplicate = candidates.iter().any(|candidate| {
        unsafe { automation.CompareElements(&candidate.element, &element) }
            .map(|same| same.as_bool())
            .unwrap_or(false)
    });
    if !duplicate {
        candidates.push(ElementCandidate { source, element });
    }
}

fn text_pattern_from_element(element: &IUIAutomationElement) -> Option<TextPatternProvider> {
    unsafe {
        element
            .GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
            .map(TextPatternProvider::TextPattern2)
            .or_else(|_| {
                element
                    .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                    .map(TextPatternProvider::TextPattern)
            })
            .ok()
    }
}

unsafe fn try_capture_selection(
    pattern: &TextPatternProvider,
    candidate: &ElementCandidate,
    capture: &mut WindowsUiaCapture,
) -> Result<bool, String> {
    let selection = unsafe { pattern.selection() }.map_err(|error| {
        format!(
            "{} ({}) 读取 UIA 选区集合失败：{error}",
            candidate.source,
            describe_element(&candidate.element)
        )
    })?;
    let selection_count = unsafe { selection.Length() }.map_err(|error| {
        format!(
            "{} ({}) 读取 UIA 选区数量失败：{error}",
            candidate.source,
            describe_element(&candidate.element)
        )
    })?;

    if capture.selection_range_count.is_none() || selection_count > 0 {
        capture.selection_range_count = Some(selection_count);
        capture.text_pattern = Some(pattern.name());
        capture.text_pattern_source = Some(candidate.source.clone());
    }
    if selection_count <= 0 {
        return Ok(false);
    }

    let mut selected_parts = Vec::new();
    let mut rectangles = Vec::new();
    let mut context_text = None;
    for index in 0..selection_count {
        let range = unsafe { selection.GetElement(index) }
            .map_err(|error| format!("读取第 {index} 个 UIA 选区失败：{error}"))?;
        let selected_text = text_from_range(&range, MAX_SELECTED_TEXT_CHARS)?;
        if !selected_text.is_empty() {
            if context_text.is_none() {
                context_text = context_from_range(&range)?;
            }
            selected_parts.push(selected_text);
            rectangles.extend(bounding_rectangles(&range)?);
        }
    }

    if selected_parts.is_empty() {
        return Ok(false);
    }

    capture.text_pattern = Some(pattern.name());
    capture.text_pattern_source = Some(candidate.source.clone());
    capture.selection_range_count = Some(selection_count);
    capture.selected_text = Some(selected_parts.join("\n"));
    capture.context_text = context_text;
    capture.bounding_rectangle_count = rectangles.len();
    capture.anchor_rect = union_rectangles(&rectangles);
    if capture.context_text.is_none() {
        capture
            .diagnostics
            .push("已取得 selectedText，但未取得非空 Paragraph contextText。".to_string());
    }
    if capture.anchor_rect.is_none() {
        capture
            .diagnostics
            .push("已取得 selectedText，但 UIA 未返回有效选区矩形。".to_string());
    }

    Ok(true)
}

fn describe_element(element: &IUIAutomationElement) -> String {
    let control_type = unsafe { element.CurrentControlType() }
        .map(|value| value.0.to_string())
        .unwrap_or_else(|_| "?".to_string());
    let class_name = unsafe { element.CurrentClassName() }
        .map(|value| truncate_diagnostic(&value.to_string()))
        .unwrap_or_else(|_| "?".to_string());
    let name = unsafe { element.CurrentName() }
        .map(|value| truncate_diagnostic(&value.to_string()))
        .unwrap_or_else(|_| "?".to_string());
    format!("controlType={control_type}, class={class_name:?}, name={name:?}")
}

fn foreground_diagnostics(window: HWND) -> ForegroundDiagnostics {
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut process_id));
    }

    ForegroundDiagnostics {
        hwnd: window.0 as usize,
        process_id,
        executable_path: process_executable_path(process_id),
        window_title: window_title(window),
        ..ForegroundDiagnostics::default()
    }
}

fn fill_focused_element_diagnostics(
    element: &IUIAutomationElement,
    diagnostics: &mut ForegroundDiagnostics,
) {
    diagnostics.focused_element_name = unsafe { element.CurrentName() }
        .ok()
        .map(|value| truncate_diagnostic(&value.to_string()));
    diagnostics.focused_element_class_name = unsafe { element.CurrentClassName() }
        .ok()
        .map(|value| truncate_diagnostic(&value.to_string()));
    diagnostics.focused_element_framework_id = unsafe { element.CurrentFrameworkId() }
        .ok()
        .map(|value| truncate_diagnostic(&value.to_string()));
    diagnostics.focused_element_control_type = unsafe { element.CurrentControlType() }
        .ok()
        .map(|value| value.0);
}

fn window_title(window: HWND) -> Option<String> {
    let mut buffer = vec![0u16; 1024];
    let length = unsafe { GetWindowTextW(window, &mut buffer) };
    if length <= 0 {
        return None;
    }

    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

fn process_executable_path(process_id: u32) -> Option<String> {
    if process_id == 0 {
        return None;
    }

    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };

    result
        .ok()
        .map(|_| String::from_utf16_lossy(&buffer[..length as usize]))
}

fn text_from_range(range: &IUIAutomationTextRange, max_chars: i32) -> Result<String, String> {
    unsafe { range.GetText(max_chars) }
        .map(|value| normalize_text(&value.to_string()))
        .map_err(|error| format!("读取 UIA 文本失败：{error}"))
}

fn context_from_range(range: &IUIAutomationTextRange) -> Result<Option<String>, String> {
    let context_range =
        unsafe { range.Clone() }.map_err(|error| format!("复制 UIA 选区失败：{error}"))?;
    unsafe { context_range.ExpandToEnclosingUnit(TextUnit_Paragraph) }
        .map_err(|error| format!("扩展 UIA Paragraph 上下文失败：{error}"))?;
    let context = text_from_range(&context_range, MAX_CONTEXT_TEXT_CHARS)?;

    Ok((!context.is_empty()).then_some(context))
}

fn bounding_rectangles(range: &IUIAutomationTextRange) -> Result<Vec<ScreenRect>, String> {
    let safe_array = OwnedSafeArray(
        unsafe { range.GetBoundingRectangles() }
            .map_err(|error| format!("读取 UIA 选区矩形失败：{error}"))?,
    );

    if safe_array.0.is_null() {
        return Ok(Vec::new());
    }

    let dimensions = unsafe { SafeArrayGetDim(safe_array.0) };
    if dimensions != 1 {
        return Err(format!("UIA 选区矩形 SAFEARRAY 维度异常：{dimensions}"));
    }

    let lower = unsafe { SafeArrayGetLBound(safe_array.0, 1) }
        .map_err(|error| format!("读取 SAFEARRAY 下界失败：{error}"))?;
    let upper = unsafe { SafeArrayGetUBound(safe_array.0, 1) }
        .map_err(|error| format!("读取 SAFEARRAY 上界失败：{error}"))?;
    if upper < lower {
        return Ok(Vec::new());
    }

    let mut values = Vec::with_capacity((upper - lower + 1) as usize);
    for index in lower..=upper {
        let mut value = 0f64;
        unsafe { SafeArrayGetElement(safe_array.0, &index, &mut value as *mut f64 as *mut c_void) }
            .map_err(|error| format!("读取 SAFEARRAY 第 {index} 项失败：{error}"))?;
        values.push(value);
    }

    Ok(values
        .chunks_exact(4)
        .filter_map(|chunk| {
            let rect = ScreenRect {
                x: chunk[0],
                y: chunk[1],
                width: chunk[2],
                height: chunk[3],
            };
            is_valid_rect(&rect).then_some(rect)
        })
        .collect())
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}

fn truncate_diagnostic(text: &str) -> String {
    const MAX_DIAGNOSTIC_CHARS: usize = 240;

    let mut characters = text.chars();
    let prefix: String = characters.by_ref().take(MAX_DIAGNOSTIC_CHARS).collect();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn is_valid_rect(rect: &ScreenRect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
        && rect.width > 0.0
        && rect.height > 0.0
}

fn union_rectangles(rectangles: &[ScreenRect]) -> Option<ScreenRect> {
    let first = rectangles.first()?;
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x + first.width;
    let mut bottom = first.y + first.height;

    for rect in &rectangles[1..] {
        left = left.min(rect.x);
        top = top.min(rect.y);
        right = right.max(rect.x + rect.width);
        bottom = bottom.max(rect.y + rect.height);
    }

    Some(ScreenRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{normalize_text, truncate_diagnostic, union_rectangles, ScreenRect};

    #[test]
    fn normalizes_uia_text() {
        assert_eq!(normalize_text("  first\r\nsecond  "), "first\nsecond");
    }

    #[test]
    fn truncates_large_diagnostic_fields() {
        let input = "a".repeat(241);
        let result = truncate_diagnostic(&input);

        assert_eq!(result.chars().count(), 241);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn unions_selection_rectangles() {
        let result = union_rectangles(&[
            ScreenRect {
                x: 100.0,
                y: 200.0,
                width: 50.0,
                height: 20.0,
            },
            ScreenRect {
                x: 90.0,
                y: 225.0,
                width: 80.0,
                height: 20.0,
            },
        ])
        .expect("expected a union");

        assert_eq!(result.x, 90.0);
        assert_eq!(result.y, 200.0);
        assert_eq!(result.width, 80.0);
        assert_eq!(result.height, 45.0);
    }
}
