//! Quick AI 系统提示词的组合式构建。
//!
//! 结构：静态分节常量（persona → behavior → output_format → boundaries）+
//! 动态上下文插槽（`<readray_context>…</readray_context>`，当前为空），
//! 由 `build_quick_ai_system_prompt` 在运行时按"静态 → 动态"顺序组装成
//! 单条 system message。未来开启记忆/学习画像注入时只需填充
//! `QuickAiDynamicContext`，提示词文本不需要改动。
//!
//! output_format 精确对齐 `src/markdownParse.ts` 的渲染白名单：模型只会
//! 输出渲染器能展示的内容；不支持的语法（表格、HTML、四级以上标题、
//! 图片）明确列出会以纯文本显示。推理模型规则（永不返回空回答、推理
//! 不展示给用户）也在此分节，缓解 deepseek-v4-flash"纯推理零内容"边界。

/// 身份分节：一句话身份，通用助手 + 英语专长（第二人称、正面、不吹嘘）。
pub const QUICK_AI_PERSONA: &str = "You are Quick AI inside ReadRay, a general-purpose assistant with strong expertise in English learning. Answer ordinary technical, life, and general questions directly; do not force them into English-learning advice.";

/// 行为分节：专长分工 + 提问策略 + 匹配用户语言。
pub const QUICK_AI_BEHAVIOR: &str = "For English learning, exam preparation, writing, and translation, give accurate, practical, expert help. For a personalized plan that lacks essential context, you may first offer brief provisional advice, then ask only 2 to 4 necessary questions; do not ask follow-up questions for simple or well-specified requests. Match the user's language.";

/// 输出格式分节：Markdown 白名单精确对齐渲染器 + 推理模型非空规则。
pub const QUICK_AI_OUTPUT_FORMAT: &str = "Use concise Markdown to structure your answer when it helps readability. The following is rendered correctly: headings (#, ##, ###), bold (**text**), italics (*text*), strikethrough (~~text~~), inline code (`code`), fenced code blocks (```), ordered (1.) and unordered (-) lists, blockquotes (>), horizontal rules (---), and links ([text](https://...)) rendered as visible text and URL; links must use http or https only. The following is NOT rendered and will appear as plain text, so do not use it: tables (|), raw HTML tags, headings of level 4 or deeper (####), and images. Avoid complex formatting. Never return an empty answer: reasoning is an internal process and must never be shown to the user; always produce actual content, even for simple questions.";

/// 诚实边界分节：负面声明 + 正面替代 + 回退行为，不虚构事实。
pub const QUICK_AI_BOUNDARIES: &str = "You run locally inside ReadRay. You have no tools and no internet access: do not claim you can browse the web, open other apps, or call external tools. You cannot read the user's local files, learning records, or long-term memory: do not claim to remember the user's past study history, saved words, or cards from other conversations. If asked to do something you cannot do, or asked for facts you do not know, say so briefly and honestly, then offer the closest useful alternative. Do not invent or fabricate dictionary definitions, translations, or exam facts.";

/// 动态上下文插槽的标记包裹（未来注入记忆/学习画像时使用）。
pub const QUICK_AI_CONTEXT_MARKERS: &str = "<readray_context>";

/// 动态上下文的结束标记。
pub const QUICK_AI_CONTEXT_END_MARKER: &str = "</readray_context>";

/// Quick AI 的动态上下文插槽。
///
/// 本轮不注入记忆：字段保持预留，默认全部为空。未来开启记忆注入时
/// 填充这些字段（并做字节预算截断），builder 会在标记内渲染内容。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuickAiDynamicContext {
    /// 预留：用户学习画像（当前不填充）。
    pub learning_profile: Option<String>,
    /// 预留：近期记忆（当前不填充）。
    pub recent_memory: Option<String>,
}

impl QuickAiDynamicContext {
    /// 动态上下文是否为空（无内容可注入）。
    fn is_empty(&self) -> bool {
        self.learning_profile.as_deref().is_none_or(str::is_empty)
            && self.recent_memory.as_deref().is_none_or(str::is_empty)
    }
}

/// 按"静态 → 动态"顺序组装完整系统提示词。
///
/// 动态上下文始终以 `<readray_context>…</readray_context>` 标记包裹：
/// 空上下文渲染为带标记的空插槽（模型读到的是占位而非指令），
/// 非空上下文则把内容放进标记内。
pub fn build_quick_ai_system_prompt(context: &QuickAiDynamicContext) -> String {
    let mut prompt = String::new();
    use std::fmt::Write;
    write!(prompt, "{QUICK_AI_PERSONA}\n\n{QUICK_AI_BEHAVIOR}\n\n{QUICK_AI_OUTPUT_FORMAT}\n\n{QUICK_AI_BOUNDARIES}\n\n").expect("String write 不会失败");

    prompt.push_str(QUICK_AI_CONTEXT_MARKERS);
    if !context.is_empty() {
        let mut segments: Vec<&str> = Vec::new();
        if let Some(profile) = context
            .learning_profile
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            segments.push(profile);
        }
        if let Some(memory) = context
            .recent_memory
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            segments.push(memory);
        }
        write!(prompt, "{}", segments.join("\n")).expect("String write 不会失败");
    }
    prompt.push_str(QUICK_AI_CONTEXT_END_MARKER);
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built() -> String {
        build_quick_ai_system_prompt(&QuickAiDynamicContext::default())
    }

    /// 组装顺序：静态分节按 persona → behavior → output_format → boundaries
    /// 排列，动态上下文标记在最后。
    #[test]
    fn sections_are_ordered_static_then_dynamic() {
        let prompt = built();
        let persona_at = prompt.find(QUICK_AI_PERSONA).expect("persona 必须存在");
        let behavior_at = prompt.find(QUICK_AI_BEHAVIOR).expect("behavior 必须存在");
        let format_at = prompt
            .find(QUICK_AI_OUTPUT_FORMAT)
            .expect("output_format 必须存在");
        let boundaries_at = prompt
            .find(QUICK_AI_BOUNDARIES)
            .expect("boundaries 必须存在");
        let marker_at = prompt
            .find(QUICK_AI_CONTEXT_MARKERS)
            .expect("上下文标记必须存在");

        assert!(persona_at < behavior_at);
        assert!(behavior_at < format_at);
        assert!(format_at < boundaries_at);
        assert!(boundaries_at < marker_at);
        assert!(prompt.ends_with(QUICK_AI_CONTEXT_END_MARKER));
    }

    /// persona 分节：通用助手 + 英语专长，保持平衡，不强行导向英语建议。
    #[test]
    fn persona_keeps_general_help_and_english_expertise_balanced() {
        let prompt = built().to_ascii_lowercase();

        assert!(prompt.contains("quick ai inside readray"));
        assert!(prompt.contains("general-purpose assistant"));
        assert!(prompt.contains("strong expertise in english learning"));
        assert!(prompt.contains("do not force them into english-learning advice"));
    }

    /// 行为分节：英语专长分工、提问策略（2-4 个必要问题）、语言匹配。
    #[test]
    fn behavior_covers_expertise_and_questioning_strategy() {
        let prompt = built().to_ascii_lowercase();

        assert!(prompt.contains("exam preparation"));
        assert!(prompt.contains("writing, and translation"));
        assert!(prompt.contains("2 to 4 necessary questions"));
        assert!(prompt.contains("brief provisional advice"));
        assert!(prompt.contains("simple or well-specified requests"));
        assert!(prompt.contains("match the user's language"));
    }

    /// 诚实边界：负面声明 + 正面替代 + 回退行为 + 不虚构。
    #[test]
    fn boundaries_combine_negative_positive_and_fallback() {
        let prompt = built().to_ascii_lowercase();

        // 负面声明
        assert!(prompt.contains("no tools and no internet access"));
        assert!(prompt.contains(
            "do not claim you can browse the web, open other apps, or call external tools"
        ));
        assert!(prompt
            .contains("cannot read the user's local files, learning records, or long-term memory"));
        assert!(prompt.contains("do not claim to remember the user's past study history, saved words, or cards from other conversations"));

        // 正面替代 + 回退行为
        assert!(prompt
            .contains("say so briefly and honestly, then offer the closest useful alternative"));
        assert!(prompt.contains(
            "do not invent or fabricate dictionary definitions, translations, or exam facts"
        ));

        // 诚实边界不得声称访问本地学习记录/长期记忆
        assert!(prompt.contains("local files, learning records"));
        assert!(prompt.contains("long-term memory"));
    }

    /// output_format 白名单契约：渲染器支持的子集全部列出。
    #[test]
    fn output_format_lists_every_rendered_whitelist_syntax() {
        let prompt = built().to_ascii_lowercase();

        assert!(prompt.contains("#, ##, ###"));
        assert!(prompt.contains("bold (**text**)"));
        assert!(prompt.contains("italics (*text*)"));
        assert!(prompt.contains("strikethrough (~~text~~)"));
        assert!(prompt.contains("inline code (`code`)"));
        assert!(prompt.contains("fenced code blocks"));
        assert!(prompt.contains("ordered (1.)"));
        assert!(prompt.contains("unordered (-)"));
        assert!(prompt.contains("blockquotes (>)"));
        assert!(prompt.contains("horizontal rules (---)"));
        assert!(prompt.contains("links ([text](https://...))"));
        assert!(prompt.contains("http or https only"));
    }

    /// output_format 负面清单：表格 / HTML / 四级+ 标题 / 图片明确以纯文本显示。
    #[test]
    fn output_format_lists_plain_text_negative_syntax() {
        let prompt = built().to_ascii_lowercase();

        assert!(prompt.contains("not rendered and will appear as plain text"));
        assert!(prompt.contains("tables (|)"));
        assert!(prompt.contains("raw html tags"));
        assert!(prompt.contains("headings of level 4 or deeper (####)"));
        assert!(prompt.contains("images"));
    }

    /// 推理模型规则：永不返回空回答、推理不外显、简单问题也要产出内容。
    #[test]
    fn output_format_requires_non_empty_answers_for_reasoning_model() {
        let prompt = built().to_ascii_lowercase();

        assert!(prompt.contains("never return an empty answer"));
        assert!(prompt.contains("reasoning is an internal process"));
        assert!(prompt.contains("never be shown to the user"));
        assert!(prompt.contains("always produce actual content, even for simple questions"));
    }

    /// 上下文标记：默认（空）上下文中标记存在且中间无内容。
    #[test]
    fn context_markers_exist_and_empty_context_renders_no_content() {
        let prompt = built();

        assert!(prompt.contains(QUICK_AI_CONTEXT_MARKERS));
        assert!(prompt.contains(QUICK_AI_CONTEXT_END_MARKER));
        let start = prompt
            .find(QUICK_AI_CONTEXT_MARKERS)
            .expect("开始标记必须存在");
        let end = prompt
            .find(QUICK_AI_CONTEXT_END_MARKER)
            .expect("结束标记必须存在");
        assert_eq!(
            &prompt[start + QUICK_AI_CONTEXT_MARKERS.len()..end],
            "",
            "空上下文时标记之间必须无内容"
        );
    }

    /// 动态注入：填充上下文后内容出现在标记内，且静态分节不受影响。
    #[test]
    fn non_empty_context_renders_inside_markers() {
        let context = QuickAiDynamicContext {
            learning_profile: Some("The user is preparing for CET-6.".to_string()),
            recent_memory: Some("Saved words: amber, lucid.".to_string()),
        };
        let prompt = build_quick_ai_system_prompt(&context);

        let start = prompt
            .find(QUICK_AI_CONTEXT_MARKERS)
            .expect("开始标记必须存在");
        let end = prompt
            .find(QUICK_AI_CONTEXT_END_MARKER)
            .expect("结束标记必须存在");
        let injected = &prompt[start + QUICK_AI_CONTEXT_MARKERS.len()..end];

        assert!(injected.contains("The user is preparing for CET-6."));
        assert!(injected.contains("Saved words: amber, lucid."));
        assert!(prompt.starts_with(QUICK_AI_PERSONA), "静态分节仍在前");
        assert!(prompt.ends_with(QUICK_AI_CONTEXT_END_MARKER));
    }

    /// 组装提示词不注入日期（前缀保持稳定，避免诱导模型回答当前事件）。
    #[test]
    fn prompt_does_not_contain_any_date() {
        let prompt = built().to_ascii_lowercase();

        assert!(!prompt.contains("2026"));
        assert!(!prompt.contains("today's date"));
        assert!(!prompt.contains("current date"));
    }
}
