use crate::explanation::{classify_query_type, is_context_sensitive_word, QueryType};
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
    IUIAutomationTextPattern2, IUIAutomationTextRange, IUIAutomationTreeWalker,
    TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Document,
    TextUnit_Paragraph, UIA_TextPattern2Id, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

const MAX_SELECTED_TEXT_CHARS: i32 = 4096;
const MAX_CONTEXT_TEXT_CHARS: i32 = 4096;
const MAX_ANCESTOR_DEPTH: usize = 16;
const MAX_RAW_VIEW_ELEMENTS: usize = 512;
const MAX_RAW_VIEW_DEPTH: usize = 16;
const UIA_CAPTURE_RETRY_DELAYS_MS: [u64; 2] = [40, 80];

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    pub minimal_context: Option<String>,
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
            minimal_context: None,
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

    fn has_usable_selection(&self) -> bool {
        self.ok
            && self
                .selected_text
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
            && self.anchor_rect.is_some()
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

pub fn capture_foreground_with_retry() -> WindowsUiaCapture {
    let mut capture = capture_foreground();
    let initial_foreground_hwnd = capture.foreground.hwnd;

    if capture.has_usable_selection() {
        return capture;
    }

    for delay_ms in UIA_CAPTURE_RETRY_DELAYS_MS {
        thread::sleep(Duration::from_millis(delay_ms));
        let retry = capture_foreground();

        if initial_foreground_hwnd != 0
            && retry.foreground.hwnd != 0
            && retry.foreground.hwnd != initial_foreground_hwnd
        {
            capture
                .diagnostics
                .push("UIA 捕获重试期间前台窗口发生变化，已停止重试。".to_string());
            break;
        }

        if retry.has_usable_selection() {
            return retry;
        }

        capture = retry;
    }

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
    let mut minimal_context = None;
    for index in 0..selection_count {
        let range = unsafe { selection.GetElement(index) }
            .map_err(|error| format!("读取第 {index} 个 UIA 选区失败：{error}"))?;
        let selected_text = text_from_range(&range, MAX_SELECTED_TEXT_CHARS)?;
        if !selected_text.is_empty() {
            if context_text.is_none() {
                let context = context_from_range(&range, selection_count == 1)?;
                context_text = context.context_text;
                minimal_context = context.minimal_context;
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
    capture.minimal_context = minimal_context;
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

struct CapturedContext {
    context_text: Option<String>,
    minimal_context: Option<String>,
}

fn context_from_range(
    range: &IUIAutomationTextRange,
    has_single_selection: bool,
) -> Result<CapturedContext, String> {
    let selected = raw_text_from_range(range, MAX_SELECTED_TEXT_CHARS).ok();
    let use_document_context = selected.as_deref().is_some_and(|text| {
        let selected = clean_model_context(text);
        matches!(classify_query_type(&selected), Ok(QueryType::Word))
            && is_context_sensitive_word(&selected)
    });
    let context_range =
        unsafe { range.Clone() }.map_err(|error| format!("复制 UIA 选区失败：{error}"))?;
    if use_document_context {
        if unsafe { context_range.ExpandToEnclosingUnit(TextUnit_Document) }.is_err() {
            unsafe { context_range.ExpandToEnclosingUnit(TextUnit_Paragraph) }
                .map_err(|error| format!("扩展 UIA Document/Paragraph 上下文失败：{error}"))?;
        }
    } else {
        unsafe { context_range.ExpandToEnclosingUnit(TextUnit_Paragraph) }
            .map_err(|error| format!("扩展 UIA Paragraph 上下文失败：{error}"))?;
    }
    let context = text_from_range(&context_range, MAX_CONTEXT_TEXT_CHARS)?;
    if context.is_empty() {
        return Ok(CapturedContext {
            context_text: None,
            minimal_context: None,
        });
    }

    let before = prefix_text_from_ranges(&context_range, range).ok();
    let after = suffix_text_from_ranges(&context_range, range).ok();
    let minimal_context = match (selected.as_deref(), before.as_deref(), after.as_deref()) {
        (Some(selected), Some(before), Some(after)) => {
            derive_minimal_context(selected, &context, before, after, has_single_selection)
        }
        _ => fallback_minimal_context(&context, selected.as_deref()),
    };

    Ok(CapturedContext {
        context_text: Some(context),
        minimal_context,
    })
}

fn raw_text_from_range(range: &IUIAutomationTextRange, max_chars: i32) -> Result<String, String> {
    unsafe { range.GetText(max_chars) }
        .map(|value| value.to_string().replace("\r\n", "\n").replace('\r', "\n"))
        .map_err(|error| format!("读取 UIA 文本失败：{error}"))
}

fn prefix_text_from_ranges(
    context_range: &IUIAutomationTextRange,
    selection_range: &IUIAutomationTextRange,
) -> Result<String, String> {
    let prefix = unsafe { context_range.Clone() }
        .map_err(|error| format!("复制 UIA Paragraph 前缀失败：{error}"))?;
    unsafe {
        prefix.MoveEndpointByRange(
            TextPatternRangeEndpoint_End,
            selection_range,
            TextPatternRangeEndpoint_Start,
        )
    }
    .map_err(|error| format!("定位 UIA 选区前边界失败：{error}"))?;
    raw_text_from_range(&prefix, MAX_CONTEXT_TEXT_CHARS)
}

fn suffix_text_from_ranges(
    context_range: &IUIAutomationTextRange,
    selection_range: &IUIAutomationTextRange,
) -> Result<String, String> {
    let suffix = unsafe { context_range.Clone() }
        .map_err(|error| format!("复制 UIA Paragraph 后缀失败：{error}"))?;
    unsafe {
        suffix.MoveEndpointByRange(
            TextPatternRangeEndpoint_Start,
            selection_range,
            TextPatternRangeEndpoint_End,
        )
    }
    .map_err(|error| format!("定位 UIA 选区后边界失败：{error}"))?;
    raw_text_from_range(&suffix, MAX_CONTEXT_TEXT_CHARS)
}

fn derive_minimal_context(
    selected_text: &str,
    paragraph_text: &str,
    prefix_text: &str,
    suffix_text: &str,
    has_single_selection: bool,
) -> Option<String> {
    let paragraph = clean_model_context(paragraph_text);
    if paragraph.is_empty() {
        return None;
    }

    let selected = clean_model_context(selected_text);
    let prefix = normalize_model_context_fragment(prefix_text);
    let suffix = normalize_model_context_fragment(suffix_text);
    let exact_position = has_single_selection
        && !selected.is_empty()
        && clean_model_context(&format!("{prefix_text}{selected_text}{suffix_text}")) == paragraph;
    let Ok(query_type) = classify_query_type(&selected) else {
        return Some(paragraph);
    };

    match query_type {
        QueryType::Paragraph => None,
        QueryType::Sentence => {
            if exact_position {
                reliable_previous_sentence(&prefix)
            } else {
                None
            }
        }
        QueryType::Word | QueryType::Phrase => {
            if query_type == QueryType::Word && is_context_sensitive_word(&selected) {
                return Some(bounded_context_window(
                    &paragraph,
                    &selected,
                    &prefix,
                    &suffix,
                    exact_position,
                ));
            }
            if !exact_position {
                return Some(paragraph);
            }
            let Ok(sentence_start) = last_reliable_sentence_boundary(&prefix) else {
                return Some(paragraph);
            };
            let Ok(sentence_end) = first_reliable_sentence_boundary(&suffix) else {
                return Some(paragraph);
            };
            let prefix_part = &prefix[sentence_start.unwrap_or(0)..];
            let suffix_part = &suffix[..sentence_end.unwrap_or(suffix.len())];
            let sentence = clean_model_context(&format!("{prefix_part}{selected}{suffix_part}"));
            if sentence.contains(&selected) {
                Some(sentence)
            } else {
                Some(paragraph)
            }
        }
    }
}

fn bounded_context_window(
    paragraph: &str,
    selected: &str,
    prefix: &str,
    suffix: &str,
    exact_position: bool,
) -> String {
    let max_chars = MAX_CONTEXT_TEXT_CHARS as usize;
    let selected_chars = selected.chars().count();
    if selected_chars >= max_chars {
        return selected.chars().take(max_chars).collect();
    }

    let reconstructed = if exact_position {
        None
    } else {
        Some(clean_model_context(&format!("{prefix}{selected}{suffix}")))
    };
    let source = reconstructed.as_deref().unwrap_or(paragraph);
    if source.chars().count() <= max_chars {
        return source.to_string();
    }

    let prefix = normalize_model_context_fragment(prefix);
    let prefix_without_leading_whitespace = prefix.trim_start();
    let total_chars = source.chars().count();
    let selection_start = (!prefix_without_leading_whitespace.is_empty()
        && source.starts_with(prefix_without_leading_whitespace))
    .then_some(prefix_without_leading_whitespace.chars().count())
    .or_else(|| {
        source
            .find(selected)
            .map(|index| source[..index].chars().count())
    })
    .unwrap_or(0)
    .min(total_chars.saturating_sub(selected_chars));
    let selection_end = selection_start + selected_chars;
    let remaining = max_chars - selected_chars;
    let desired_before = remaining / 2;
    let before = desired_before.min(selection_start);
    let after = (remaining - before).min(total_chars - selection_end);
    let extra_before = (remaining - before - after).min(selection_start - before);
    let before = before + extra_before;
    let extra_after = (remaining - before - after).min(total_chars - selection_end - after);
    let after = after + extra_after;
    let window_start = selection_start - before;
    let window_end = selection_end + after;
    let start_byte = source
        .char_indices()
        .nth(window_start)
        .map(|(index, _)| index)
        .unwrap_or(source.len());
    let end_byte = source
        .char_indices()
        .nth(window_end)
        .map(|(index, _)| index)
        .unwrap_or(source.len());

    source[start_byte..end_byte].to_string()
}

fn fallback_minimal_context(paragraph_text: &str, selected_text: Option<&str>) -> Option<String> {
    let paragraph = clean_model_context(paragraph_text);
    let query_type = selected_text
        .map(clean_model_context)
        .filter(|selected| !selected.is_empty())
        .and_then(|selected| classify_query_type(&selected).ok());
    match query_type {
        Some(QueryType::Sentence | QueryType::Paragraph) => None,
        _ => (!paragraph.is_empty()).then_some(paragraph),
    }
}

fn clean_model_context(text: &str) -> String {
    normalize_model_context_fragment(text).trim().to_string()
}

fn normalize_model_context_fragment(text: &str) -> String {
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{200b}', "")
        .replace('\u{fffc}', "");
    let mut lines = Vec::new();
    let mut previous_was_blank = false;
    for line in normalized.split('\n') {
        let is_blank = line.trim().is_empty();
        if is_blank {
            if !previous_was_blank {
                lines.push("");
            }
        } else {
            lines.push(line);
        }
        previous_was_blank = is_blank;
    }
    lines.join("\n")
}

fn reliable_previous_sentence(prefix: &str) -> Option<String> {
    let prefix = prefix.trim_end();
    if prefix.is_empty() {
        return None;
    }
    let boundaries = reliable_sentence_boundaries(prefix).ok()?;
    let last = *boundaries.last()?;
    if last != prefix.len() {
        return None;
    }
    let start = if boundaries.len() >= 2 {
        boundaries[boundaries.len() - 2]
    } else {
        0
    };
    let sentence = clean_model_context(&prefix[start..last]);
    (!sentence.is_empty()).then_some(sentence)
}

fn last_reliable_sentence_boundary(text: &str) -> Result<Option<usize>, ()> {
    Ok(reliable_sentence_boundaries(text)?.last().copied())
}

fn first_reliable_sentence_boundary(text: &str) -> Result<Option<usize>, ()> {
    Ok(reliable_sentence_boundaries(text)?.first().copied())
}

fn reliable_sentence_boundaries(text: &str) -> Result<Vec<usize>, ()> {
    let mut boundaries = Vec::new();
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = 0;
    while index < characters.len() {
        let (byte_index, character) = characters[index];
        let mut boundary_end = byte_index + character.len_utf8();
        match character {
            '。' | '！' | '？' | '!' | '?' | '\n' => {}
            '.' => {
                if !ascii_period_is_reliable(text, byte_index) {
                    return Err(());
                }
            }
            _ => {
                index += 1;
                continue;
            }
        }

        let mut next = index + 1;
        while let Some((closing_index, closing)) = characters.get(next).copied() {
            if matches!(
                closing,
                '"' | '\'' | '”' | '’' | ')' | ']' | '}' | '》' | '」' | '』'
            ) {
                boundary_end = closing_index + closing.len_utf8();
                next += 1;
            } else {
                break;
            }
        }
        boundaries.push(boundary_end);
        index = next;
    }
    Ok(boundaries)
}

fn ascii_period_is_reliable(text: &str, byte_index: usize) -> bool {
    let before = &text[..byte_index];
    let after = &text[byte_index + 1..];
    let previous = before.chars().next_back();
    let next = after.chars().next();
    if previous.is_some_and(|value| value.is_ascii_digit())
        && next.is_some_and(|value| value.is_ascii_digit())
    {
        return false;
    }
    if next.is_some_and(|value| {
        !value.is_whitespace() && !matches!(value, '"' | '\'' | '”' | '’' | ')' | ']' | '}')
    }) {
        return false;
    }
    let token: String = before
        .chars()
        .rev()
        .take_while(|value| value.is_ascii_alphabetic())
        .collect();
    !(1..=3).contains(&token.len())
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
    use super::{
        clean_model_context, derive_minimal_context, normalize_text, reliable_previous_sentence,
        truncate_diagnostic, union_rectangles, ScreenRect,
    };

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

    #[test]
    fn derives_word_and_phrase_context_from_the_exact_selection_position() {
        let repeated = derive_minimal_context(
            "market",
            "The first market closed. The second market remained open.",
            "The first market closed. The second ",
            " remained open.",
            true,
        );
        assert_eq!(
            repeated.as_deref(),
            Some("The second market remained open.")
        );

        let phrase = derive_minimal_context(
            "in progress",
            "前一项已完成。The migration is still in progress! 后一项稍后开始。",
            "前一项已完成。The migration is still ",
            "! 后一项稍后开始。",
            true,
        );
        assert_eq!(
            phrase.as_deref(),
            Some("The migration is still in progress!")
        );
    }

    #[test]
    fn context_sensitive_single_tokens_keep_later_disambiguating_context() {
        let cases = [
            (
                "XYZ",
                "A note mentions XYZ first. In this context, XYZ refers to the deployment role.",
                "A note mentions ",
                " first. In this context, XYZ refers to the deployment role.",
            ),
            (
                "U.S.",
                "The text mentions U.S. first. The surrounding paragraph explains the intended jurisdiction.",
                "The text mentions ",
                " first. The surrounding paragraph explains the intended jurisdiction.",
            ),
            (
                "node.js",
                "The note mentions node.js first. The next sentence identifies the runtime used here.",
                "The note mentions ",
                " first. The next sentence identifies the runtime used here.",
            ),
        ];

        for (selected, paragraph, prefix, suffix) in cases {
            assert_eq!(
                derive_minimal_context(selected, paragraph, prefix, suffix, true).as_deref(),
                Some(paragraph),
                "{selected} should retain paragraph context"
            );
        }
    }

    #[test]
    fn oversized_context_sensitive_context_is_bounded_and_keeps_selection_window() {
        let prefix = format!("{}A note mentions ", "before ".repeat(250));
        let suffix = format!(
            " first. In this context, XYZ refers to the deployment role.{}",
            " after ".repeat(250)
        );
        let paragraph = format!("{prefix}XYZ{suffix}");
        let context = derive_minimal_context("XYZ", &paragraph, &prefix, &suffix, true)
            .expect("expected bounded context");

        assert!(context.chars().count() <= 4096);
        assert!(context.contains("XYZ"));
        assert!(context.contains("deployment role"));
    }

    #[test]
    fn truncated_paragraph_reconstructs_context_around_the_selected_token() {
        let prefix = "before ".repeat(700);
        let suffix = " after ".repeat(700);
        let context = derive_minimal_context(
            "XYZ",
            "The captured paragraph prefix is truncated before the selection.",
            &prefix,
            &format!(" first. In this context, XYZ refers to the deployment role.{suffix}"),
            true,
        )
        .expect("expected reconstructed context");

        assert!(context.chars().count() <= 4096);
        assert!(context.contains("XYZ"));
        assert!(context.contains("deployment role"));
    }

    #[test]
    fn sentence_context_uses_only_a_reliable_previous_sentence() {
        assert_eq!(
            reliable_previous_sentence("The cache is local.").as_deref(),
            Some("The cache is local.")
        );
        let context = derive_minimal_context(
            "It remains available.",
            "The cache is local. It remains available. A later sentence follows.",
            "The cache is local. ",
            " A later sentence follows.",
            true,
        );
        assert_eq!(context.as_deref(), Some("The cache is local."));

        let unavailable = derive_minimal_context(
            "This remains available.",
            "An unfinished lead-in this remains available.",
            "An unfinished lead-in ",
            "",
            true,
        );
        assert_eq!(unavailable, None);
    }

    #[test]
    fn paragraph_context_is_never_duplicated() {
        let paragraph = "The first sentence is here. The second sentence is also here.";
        assert_eq!(
            derive_minimal_context(paragraph, paragraph, "", "", false),
            None
        );
    }

    #[test]
    fn uncertain_boundaries_fall_back_to_the_cleaned_paragraph() {
        let paragraph = "Mr. Smith opened the first line\nwithout a reliable sentence ending";
        let context = derive_minimal_context(
            "first",
            paragraph,
            "Mr. Smith opened the ",
            " line\nwithout a reliable sentence ending",
            true,
        );
        assert_eq!(context.as_deref(), Some(paragraph));

        let uncertain_position = derive_minimal_context(
            "target",
            "prefix target suffix",
            "unrelated ",
            " suffix",
            true,
        );
        assert_eq!(uncertain_position.as_deref(), Some("prefix target suffix"));
    }

    #[test]
    fn deterministic_cleanup_removes_only_known_uia_noise() {
        assert_eq!(
            clean_model_context("  first\r\n\u{200b}\u{fffc}\n\n\nsecond  "),
            "first\n\nsecond"
        );
        assert_eq!(
            derive_minimal_context(
                "目标",
                "上一句。这里是目标！下一句。",
                "上一句。这里是",
                "！下一句。",
                true,
            )
            .as_deref(),
            Some("这里是目标！")
        );
    }
}
