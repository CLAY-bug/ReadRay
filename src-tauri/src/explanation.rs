use serde::{Deserialize, Serialize};

pub const MAX_QUERY_TEXT_LEN: usize = 4_096;
pub(crate) const MAX_CONTEXT_TEXT_LEN: usize = 4_096;
const MAX_SOURCE_APP_LEN: usize = 512;
const MAX_SOURCE_TEXT_LEN: usize = 4_096;
pub const MAX_LEARNING_TARGET_TEXT_LEN: usize = 4_096;
const MAX_HEADWORD_LEN: usize = 160;
const MAX_PART_OF_SPEECH_LEN: usize = 80;
const MAX_PHONETIC_LEN: usize = 80;
const MAX_BASIC_MEANINGS: usize = 4;
const MAX_BASIC_MEANING_LEN: usize = 400;
const MAX_CONTEXT_MEANING_LEN: usize = 800;
const MAX_SOURCE_SENTENCE_LEN: usize = 1_200;
const MAX_SOURCE_SENTENCE_ZH_LEN: usize = 2_400;
const MAX_PHRASES: usize = 3;
const MAX_NEAR_MEANINGS: usize = 3;
const MAX_EXAMPLES: usize = 2;
const MAX_PHRASE_TEXT_LEN: usize = 160;
const MAX_ITEM_MEANING_LEN: usize = 400;
const MAX_EXAMPLE_EN_LEN: usize = 600;
const MAX_EXAMPLE_ZH_LEN: usize = 1_000;
const MAX_COMPOSITION_LEN: usize = 800;
const MAX_SENTENCE_KEY_POINTS: usize = 3;
const MAX_PARAGRAPH_KEY_POINTS: usize = 5;
const MAX_KEY_POINT_LEN: usize = 600;
const MAX_TRANSLATION_LEN: usize = 12_000;
const MAX_EXPLANATION_LEN: usize = 1_600;
const MAX_SUMMARY_LEN: usize = 1_000;
const MAX_REVIEW_HINT_LEN: usize = 400;
const PARAGRAPH_CHAR_THRESHOLD: usize = 320;
const PARAGRAPH_WORD_THRESHOLD: usize = 55;
const SENTENCE_WORD_THRESHOLD: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryType {
    Word,
    Phrase,
    Sentence,
    Paragraph,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QueryDirection {
    EnToZh,
    ZhToEn,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Manual,
    Clipboard,
    WindowsUia,
    AppAdapter,
    Ocr,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInput {
    pub query_text: String,
    pub context_text: Option<String>,
    pub source_type: SourceType,
    #[serde(default)]
    pub source_app: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "queryType",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ExplanationCard {
    Word {
        source_text: String,
        #[serde(default)]
        learning_target_text: String,
        headword: String,
        part_of_speech: Option<String>,
        phonetic: Option<String>,
        basic_meanings: Vec<String>,
        context_meaning: Option<String>,
        source_sentence: Option<String>,
        source_sentence_zh: Option<String>,
        #[serde(default)]
        phrases: Vec<PhraseItem>,
        #[serde(default)]
        near_meanings: Vec<NearMeaningItem>,
        #[serde(default)]
        examples: Vec<ExampleItem>,
        review_hint: Option<String>,
    },
    Phrase {
        source_text: String,
        #[serde(default)]
        learning_target_text: String,
        basic_meaning: String,
        context_meaning: Option<String>,
        composition: Option<String>,
        source_sentence: Option<String>,
        source_sentence_zh: Option<String>,
        #[serde(default)]
        examples: Vec<ExampleItem>,
        review_hint: Option<String>,
    },
    Sentence {
        source_text: String,
        #[serde(default)]
        learning_target_text: String,
        translation: String,
        #[serde(default)]
        key_points: Vec<KeyPointItem>,
        explanation: Option<String>,
        review_hint: Option<String>,
    },
    Paragraph {
        source_text: String,
        #[serde(default)]
        learning_target_text: String,
        translation: String,
        #[serde(default)]
        key_points: Vec<KeyPointItem>,
        summary: Option<String>,
    },
}

impl ExplanationCard {
    pub fn query_type(&self) -> QueryType {
        match self {
            Self::Word { .. } => QueryType::Word,
            Self::Phrase { .. } => QueryType::Phrase,
            Self::Sentence { .. } => QueryType::Sentence,
            Self::Paragraph { .. } => QueryType::Paragraph,
        }
    }

    pub fn source_text(&self) -> &str {
        match self {
            Self::Word { source_text, .. }
            | Self::Phrase { source_text, .. }
            | Self::Sentence { source_text, .. }
            | Self::Paragraph { source_text, .. } => source_text,
        }
    }

    pub(crate) fn set_source_text(&mut self, value: String) {
        match self {
            Self::Word { source_text, .. }
            | Self::Phrase { source_text, .. }
            | Self::Sentence { source_text, .. }
            | Self::Paragraph { source_text, .. } => *source_text = value,
        }
    }

    pub fn learning_target_text(&self) -> &str {
        match self {
            Self::Word {
                learning_target_text,
                ..
            }
            | Self::Phrase {
                learning_target_text,
                ..
            }
            | Self::Sentence {
                learning_target_text,
                ..
            }
            | Self::Paragraph {
                learning_target_text,
                ..
            } => learning_target_text,
        }
    }

    pub(crate) fn set_learning_target_text(&mut self, value: String) {
        match self {
            Self::Word {
                learning_target_text,
                ..
            }
            | Self::Phrase {
                learning_target_text,
                ..
            }
            | Self::Sentence {
                learning_target_text,
                ..
            }
            | Self::Paragraph {
                learning_target_text,
                ..
            } => *learning_target_text = value,
        }
    }

    pub(crate) fn align_primary_result_with_learning_target(&mut self) {
        let target = self.learning_target_text().trim().to_string();
        match self {
            Self::Word { headword, .. } => *headword = target,
            Self::Phrase { basic_meaning, .. } => *basic_meaning = target,
            Self::Sentence { translation, .. } | Self::Paragraph { translation, .. } => {
                *translation = target
            }
        }
    }
}

fn is_han(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2ebef}'
            | '\u{2f800}'..='\u{2fa1f}'
            | '\u{30000}'..='\u{3134f}'
    )
}

pub fn determine_query_direction(query_text: &str) -> Result<QueryDirection, String> {
    let value = query_text.trim();
    if value
        .chars()
        .any(|character| character.is_ascii_alphabetic())
    {
        return Ok(QueryDirection::EnToZh);
    }
    if value.chars().any(is_han) {
        return Ok(QueryDirection::ZhToEn);
    }
    Err("captureInput.queryText 必须包含英文或中文文字，不能只有数字、符号或空白。".to_string())
}

pub fn normalize_english_learning_target(query_text: &str) -> Result<String, String> {
    if !query_text.chars().any(is_han) {
        let normalized = query_text.split_whitespace().collect::<Vec<_>>().join(" ");
        validate_english_learning_target(&normalized)?;
        return Ok(normalized);
    }
    let mut retained = String::with_capacity(query_text.len());
    for character in query_text.trim().chars() {
        if is_han(character) {
            retained.push(' ');
        } else if character.is_ascii_alphanumeric()
            || character.is_ascii_whitespace()
            || matches!(
                character,
                '_' | '-'
                    | '\''
                    | '.'
                    | '/'
                    | '+'
                    | '#'
                    | ':'
                    | '@'
                    | '$'
                    | '%'
                    | '&'
                    | '*'
                    | '='
                    | '<'
                    | '>'
                    | '['
                    | ']'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '\\'
                    | '`'
                    | '~'
            )
        {
            retained.push(character);
        } else {
            retained.push(' ');
        }
    }
    let normalized = retained.split_whitespace().collect::<Vec<_>>().join(" ");
    validate_english_learning_target(&normalized)?;
    Ok(normalized)
}

pub(crate) fn normalize_model_english_learning_target(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn validate_english_learning_target(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        || value.chars().any(is_han)
    {
        return Err("learningTargetText 必须包含有效拉丁英文且不得含中文。".to_string());
    }
    let len = value.chars().count();
    if len > MAX_LEARNING_TARGET_TEXT_LEN {
        return Err(format!(
            "learningTargetText 长度不能超过 {MAX_LEARNING_TARGET_TEXT_LEN} 个字符，当前为 {len}。"
        ));
    }
    Ok(())
}

pub(crate) fn is_primarily_chinese_source_sentence(value: &str) -> bool {
    let mut han_count = 0_usize;
    for character in value.chars() {
        if matches!(
            character,
            '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
        ) {
            han_count += 1;
        }
    }

    // 中英混排（如 "GLM Coding 7天体验卡"）在技术阅读场景是常态，任何人类都
    // 会把它读成中文句子；只要含汉字就不再强制提供中译，纯拉丁原句才要求翻译。
    han_count > 0
}

fn source_sentence_fields_mut(
    card: &mut ExplanationCard,
) -> Option<(&mut Option<String>, &mut Option<String>)> {
    match card {
        ExplanationCard::Word {
            source_sentence,
            source_sentence_zh,
            ..
        }
        | ExplanationCard::Phrase {
            source_sentence,
            source_sentence_zh,
            ..
        } => Some((source_sentence, source_sentence_zh)),
        ExplanationCard::Sentence { .. } | ExplanationCard::Paragraph { .. } => None,
    }
}

pub(crate) fn normalize_source_sentence_translation(card: &mut ExplanationCard) {
    let Some((source_sentence, source_sentence_zh)) = source_sentence_fields_mut(card) else {
        return;
    };

    if source_sentence
        .as_deref()
        .is_some_and(is_primarily_chinese_source_sentence)
    {
        *source_sentence_zh = None;
    }
}

pub(crate) fn clear_source_sentence_fields(card: &mut ExplanationCard) -> bool {
    let Some((source_sentence, source_sentence_zh)) = source_sentence_fields_mut(card) else {
        return false;
    };

    let had_any = source_sentence.is_some() || source_sentence_zh.is_some();
    *source_sentence = None;
    *source_sentence_zh = None;
    had_any
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseItem {
    pub phrase: String,
    pub meaning: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearMeaningItem {
    pub term: String,
    pub meaning: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExampleItem {
    pub en: String,
    pub zh: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPointItem {
    pub expression: String,
    pub meaning: String,
}

pub fn classify_query_type(query_text: &str) -> Result<QueryType, String> {
    let value = query_text.trim();
    if value.is_empty() {
        return Err("captureInput.queryText 不能为空。".to_string());
    }

    let char_count = value.chars().count();
    if char_count > MAX_QUERY_TEXT_LEN {
        return Err(format!(
            "captureInput.queryText 长度不能超过 {MAX_QUERY_TEXT_LEN} 个字符，当前为 {char_count}。"
        ));
    }

    let direction = determine_query_direction(value)?;
    let han_count = value.chars().filter(|character| is_han(*character)).count();
    if direction == QueryDirection::ZhToEn && is_word_like(value) && han_count <= 4 {
        return Ok(QueryType::Word);
    }
    if direction == QueryDirection::EnToZh && is_word_like(value) {
        return Ok(QueryType::Word);
    }

    let word_count = value.split_whitespace().count();
    let sentence_terminators = value
        .chars()
        .filter(|character| matches!(character, '.' | '!' | '?' | '。' | '！' | '？'))
        .count();
    if value.contains(['\n', '\r'])
        || sentence_terminators >= 2
        || (direction == QueryDirection::ZhToEn && char_count >= 120)
        || char_count >= PARAGRAPH_CHAR_THRESHOLD
        || word_count >= PARAGRAPH_WORD_THRESHOLD
    {
        return Ok(QueryType::Paragraph);
    }

    if sentence_terminators == 1
        || word_count >= SENTENCE_WORD_THRESHOLD
        || (direction == QueryDirection::ZhToEn && han_count >= 12)
    {
        return Ok(QueryType::Sentence);
    }

    Ok(QueryType::Phrase)
}

pub fn validate_explanation_card(
    input: &CaptureInput,
    card: &ExplanationCard,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let expected_query_type = validate_capture_input(&mut errors, input);

    validate_required_text(
        &mut errors,
        "explanationCard.sourceText",
        card.source_text(),
    );
    validate_text_len(
        &mut errors,
        "explanationCard.sourceText",
        card.source_text(),
        MAX_SOURCE_TEXT_LEN,
    );
    if !input.query_text.trim().is_empty() && card.source_text() != input.query_text.trim() {
        errors.push(
            "explanationCard.sourceText 必须原样保留 captureInput.queryText（忽略首尾空白）。"
                .to_string(),
        );
    }
    if expected_query_type.is_some_and(|expected| expected != card.query_type()) {
        errors.push(format!(
            "explanationCard.queryType 与本地分类不一致：期望 {}，实际 {}。",
            query_type_label(expected_query_type.unwrap()),
            query_type_label(card.query_type())
        ));
    }
    match determine_query_direction(&input.query_text) {
        Ok(QueryDirection::EnToZh) => match normalize_english_learning_target(&input.query_text) {
            Ok(expected) if card.learning_target_text() != expected => errors.push(
                "explanationCard.learningTargetText 必须等于 Rust 本地规范化的英文查询。"
                    .to_string(),
            ),
            Err(error) => errors.push(error),
            _ => {}
        },
        Ok(QueryDirection::ZhToEn) => {
            if let Err(error) = validate_english_learning_target(card.learning_target_text()) {
                errors.push(format!("explanationCard.{error}"));
            }
        }
        Err(error) => {
            if !errors.contains(&error) {
                errors.push(error);
            }
        }
    }

    match card {
        ExplanationCard::Word {
            headword,
            part_of_speech,
            phonetic,
            basic_meanings,
            context_meaning,
            source_sentence,
            source_sentence_zh,
            phrases,
            near_meanings,
            examples,
            review_hint,
            ..
        } => {
            validate_required_text(&mut errors, "explanationCard.headword", headword);
            validate_text_len(
                &mut errors,
                "explanationCard.headword",
                headword,
                MAX_HEADWORD_LEN,
            );
            validate_optional_text_len(
                &mut errors,
                "explanationCard.partOfSpeech",
                part_of_speech.as_deref(),
                MAX_PART_OF_SPEECH_LEN,
            );
            validate_optional_text_len(
                &mut errors,
                "explanationCard.phonetic",
                phonetic.as_deref(),
                MAX_PHONETIC_LEN,
            );
            validate_string_array(
                &mut errors,
                "explanationCard.basicMeanings",
                basic_meanings,
                1,
                MAX_BASIC_MEANINGS,
                MAX_BASIC_MEANING_LEN,
            );
            validate_context_fields(
                &mut errors,
                input,
                context_meaning.as_deref(),
                source_sentence.as_deref(),
                source_sentence_zh.as_deref(),
            );
            validate_phrase_items(&mut errors, phrases);
            validate_near_meaning_items(&mut errors, near_meanings);
            validate_examples(&mut errors, examples);
            validate_optional_text_len(
                &mut errors,
                "explanationCard.reviewHint",
                review_hint.as_deref(),
                MAX_REVIEW_HINT_LEN,
            );
        }
        ExplanationCard::Phrase {
            basic_meaning,
            context_meaning,
            composition,
            source_sentence,
            source_sentence_zh,
            examples,
            review_hint,
            ..
        } => {
            validate_required_text(&mut errors, "explanationCard.basicMeaning", basic_meaning);
            validate_text_len(
                &mut errors,
                "explanationCard.basicMeaning",
                basic_meaning,
                MAX_BASIC_MEANING_LEN,
            );
            validate_context_fields(
                &mut errors,
                input,
                context_meaning.as_deref(),
                source_sentence.as_deref(),
                source_sentence_zh.as_deref(),
            );
            validate_optional_text_len(
                &mut errors,
                "explanationCard.composition",
                composition.as_deref(),
                MAX_COMPOSITION_LEN,
            );
            validate_examples(&mut errors, examples);
            validate_optional_text_len(
                &mut errors,
                "explanationCard.reviewHint",
                review_hint.as_deref(),
                MAX_REVIEW_HINT_LEN,
            );
        }
        ExplanationCard::Sentence {
            translation,
            key_points,
            explanation,
            review_hint,
            ..
        } => {
            validate_required_text(&mut errors, "explanationCard.translation", translation);
            validate_text_len(
                &mut errors,
                "explanationCard.translation",
                translation,
                MAX_TRANSLATION_LEN,
            );
            validate_key_points(
                &mut errors,
                key_points,
                "explanationCard.keyPoints",
                MAX_SENTENCE_KEY_POINTS,
            );
            validate_optional_text_len(
                &mut errors,
                "explanationCard.explanation",
                explanation.as_deref(),
                MAX_EXPLANATION_LEN,
            );
            validate_optional_text_len(
                &mut errors,
                "explanationCard.reviewHint",
                review_hint.as_deref(),
                MAX_REVIEW_HINT_LEN,
            );
        }
        ExplanationCard::Paragraph {
            translation,
            key_points,
            summary,
            ..
        } => {
            validate_required_text(&mut errors, "explanationCard.translation", translation);
            validate_text_len(
                &mut errors,
                "explanationCard.translation",
                translation,
                MAX_TRANSLATION_LEN,
            );
            validate_key_points(
                &mut errors,
                key_points,
                "explanationCard.keyPoints",
                MAX_PARAGRAPH_KEY_POINTS,
            );
            validate_optional_text_len(
                &mut errors,
                "explanationCard.summary",
                summary.as_deref(),
                MAX_SUMMARY_LEN,
            );
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_capture_input(errors: &mut Vec<String>, input: &CaptureInput) -> Option<QueryType> {
    validate_required_text(errors, "captureInput.queryText", &input.query_text);
    validate_text_len(
        errors,
        "captureInput.queryText",
        &input.query_text,
        MAX_QUERY_TEXT_LEN,
    );
    validate_optional_text_len(
        errors,
        "captureInput.contextText",
        input.context_text.as_deref(),
        MAX_CONTEXT_TEXT_LEN,
    );
    validate_optional_text_len(
        errors,
        "captureInput.sourceApp",
        input.source_app.as_deref(),
        MAX_SOURCE_APP_LEN,
    );

    match classify_query_type(&input.query_text) {
        Ok(query_type) => Some(query_type),
        Err(error) => {
            if !errors.contains(&error) {
                errors.push(error);
            }
            None
        }
    }
}

fn validate_context_fields(
    errors: &mut Vec<String>,
    input: &CaptureInput,
    context_meaning: Option<&str>,
    source_sentence: Option<&str>,
    source_sentence_zh: Option<&str>,
) {
    validate_optional_text_len(
        errors,
        "explanationCard.contextMeaning",
        context_meaning,
        MAX_CONTEXT_MEANING_LEN,
    );
    validate_optional_text_len(
        errors,
        "explanationCard.sourceSentence",
        source_sentence,
        MAX_SOURCE_SENTENCE_LEN,
    );
    validate_optional_text_len(
        errors,
        "explanationCard.sourceSentenceZh",
        source_sentence_zh,
        MAX_SOURCE_SENTENCE_ZH_LEN,
    );

    let has_context = input
        .context_text
        .as_deref()
        .is_some_and(|value| !is_blank(value));
    if !has_context && context_meaning.is_some_and(|value| !is_blank(value)) {
        errors.push(
            "explanationCard.contextMeaning 只有在 captureInput.contextText 存在时才允许提供。"
                .to_string(),
        );
    }
    if source_sentence.is_none() && source_sentence_zh.is_some() {
        errors.push(
            "explanationCard.sourceSentenceZh 只有在 sourceSentence 存在时才允许提供。".to_string(),
        );
    } else if source_sentence.is_some_and(|value| !is_primarily_chinese_source_sentence(value))
        && source_sentence_zh.is_none()
    {
        errors.push(
            "非中文主导的 explanationCard.sourceSentence 必须同时提供 sourceSentenceZh。"
                .to_string(),
        );
    }
}

fn validate_phrase_items(errors: &mut Vec<String>, items: &[PhraseItem]) {
    validate_array_len(
        errors,
        "explanationCard.phrases",
        items.len(),
        0,
        MAX_PHRASES,
    );
    for (index, item) in items.iter().enumerate() {
        let path = format!("explanationCard.phrases[{index}]");
        validate_required_text(errors, &format!("{path}.phrase"), &item.phrase);
        validate_text_len(
            errors,
            &format!("{path}.phrase"),
            &item.phrase,
            MAX_PHRASE_TEXT_LEN,
        );
        validate_required_text(errors, &format!("{path}.meaning"), &item.meaning);
        validate_text_len(
            errors,
            &format!("{path}.meaning"),
            &item.meaning,
            MAX_ITEM_MEANING_LEN,
        );
    }
}

fn validate_near_meaning_items(errors: &mut Vec<String>, items: &[NearMeaningItem]) {
    validate_array_len(
        errors,
        "explanationCard.nearMeanings",
        items.len(),
        0,
        MAX_NEAR_MEANINGS,
    );
    for (index, item) in items.iter().enumerate() {
        let path = format!("explanationCard.nearMeanings[{index}]");
        validate_required_text(errors, &format!("{path}.term"), &item.term);
        validate_text_len(
            errors,
            &format!("{path}.term"),
            &item.term,
            MAX_PHRASE_TEXT_LEN,
        );
        validate_required_text(errors, &format!("{path}.meaning"), &item.meaning);
        validate_text_len(
            errors,
            &format!("{path}.meaning"),
            &item.meaning,
            MAX_ITEM_MEANING_LEN,
        );
    }
}

fn validate_examples(errors: &mut Vec<String>, examples: &[ExampleItem]) {
    validate_array_len(
        errors,
        "explanationCard.examples",
        examples.len(),
        0,
        MAX_EXAMPLES,
    );
    for (index, item) in examples.iter().enumerate() {
        let path = format!("explanationCard.examples[{index}]");
        validate_required_text(errors, &format!("{path}.en"), &item.en);
        validate_text_len(errors, &format!("{path}.en"), &item.en, MAX_EXAMPLE_EN_LEN);
        validate_required_text(errors, &format!("{path}.zh"), &item.zh);
        validate_text_len(errors, &format!("{path}.zh"), &item.zh, MAX_EXAMPLE_ZH_LEN);
    }
}

fn validate_key_points(
    errors: &mut Vec<String>,
    items: &[KeyPointItem],
    field: &str,
    max_len: usize,
) {
    validate_array_len(errors, field, items.len(), 0, max_len);
    for (index, item) in items.iter().enumerate() {
        let path = format!("{field}[{index}]");
        validate_required_text(errors, &format!("{path}.expression"), &item.expression);
        validate_text_len(
            errors,
            &format!("{path}.expression"),
            &item.expression,
            MAX_PHRASE_TEXT_LEN,
        );
        validate_required_text(errors, &format!("{path}.meaning"), &item.meaning);
        validate_text_len(
            errors,
            &format!("{path}.meaning"),
            &item.meaning,
            MAX_KEY_POINT_LEN,
        );
    }
}

fn validate_string_array(
    errors: &mut Vec<String>,
    field: &str,
    items: &[String],
    min_len: usize,
    max_len: usize,
    max_item_len: usize,
) {
    validate_array_len(errors, field, items.len(), min_len, max_len);
    for (index, item) in items.iter().enumerate() {
        let path = format!("{field}[{index}]");
        validate_required_text(errors, &path, item);
        validate_text_len(errors, &path, item, max_item_len);
    }
}

fn is_word_like(value: &str) -> bool {
    let basic_word_like = !value.chars().any(char::is_whitespace)
        && value.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '-' | '\'' | '’')
        });

    basic_word_like || is_identifier_like(value)
}

pub(crate) fn is_context_sensitive_word(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return false;
    }

    if is_identifier_like(value) || value.contains('_') {
        return true;
    }

    let ascii_letters = value
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .count();
    let uppercase_letters = value
        .chars()
        .filter(|character| character.is_ascii_uppercase())
        .count();
    let has_digit = value.chars().any(|character| character.is_ascii_digit());
    let has_lowercase = value
        .chars()
        .any(|character| character.is_ascii_lowercase());
    let has_internal_uppercase = value
        .chars()
        .skip(1)
        .any(|character| character.is_ascii_uppercase());

    let all_caps_token = ascii_letters >= 2 && uppercase_letters == ascii_letters;
    let mixed_case_identifier = has_lowercase && has_internal_uppercase;
    let alphanumeric_identifier = has_digit && uppercase_letters > 0;

    all_caps_token || mixed_case_identifier || alphanumeric_identifier
}

fn is_identifier_like(value: &str) -> bool {
    if value.is_empty()
        || value.chars().any(char::is_whitespace)
        || !value
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        || !value
            .chars()
            .any(|character| matches!(character, '.' | '+' | '#'))
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '#')
        })
    {
        return false;
    }

    !value.ends_with('.') || is_dotted_initialism(value)
}

fn is_dotted_initialism(value: &str) -> bool {
    let mut segments = value.split('.').collect::<Vec<_>>();
    if segments.last() != Some(&"") {
        return false;
    }
    segments.pop();
    segments.len() >= 2
        && segments.iter().all(|segment| {
            segment.len() == 1
                && segment
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
        })
}

fn query_type_label(query_type: QueryType) -> &'static str {
    match query_type {
        QueryType::Word => "word",
        QueryType::Phrase => "phrase",
        QueryType::Sentence => "sentence",
        QueryType::Paragraph => "paragraph",
    }
}

fn validate_required_text(errors: &mut Vec<String>, field: &str, value: &str) {
    if is_blank(value) {
        errors.push(format!("{field} 不能为空。"));
    }
}

fn validate_text_len(errors: &mut Vec<String>, field: &str, value: &str, max_len: usize) {
    let len = value.chars().count();
    if len > max_len {
        errors.push(format!(
            "{field} 长度不能超过 {max_len} 个字符，当前为 {len}。"
        ));
    }
}

fn validate_optional_text_len(
    errors: &mut Vec<String>,
    field: &str,
    value: Option<&str>,
    max_len: usize,
) {
    if let Some(value) = value {
        if is_blank(value) {
            errors.push(format!("{field} 如果提供就不能为空。"));
        }
        validate_text_len(errors, field, value, max_len);
    }
}

fn validate_array_len(
    errors: &mut Vec<String>,
    field: &str,
    len: usize,
    min_len: usize,
    max_len: usize,
) {
    if len < min_len {
        errors.push(format!("{field} 至少需要 {min_len} 项，当前为 {len} 项。"));
    }
    if len > max_len {
        errors.push(format!("{field} 最多允许 {max_len} 项，当前为 {len} 项。"));
    }
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(query_text: &str, context_text: Option<&str>) -> CaptureInput {
        CaptureInput {
            query_text: query_text.to_string(),
            context_text: context_text.map(str::to_string),
            source_type: SourceType::WindowsUia,
            source_app: None,
        }
    }

    fn valid_word_card(context_meaning: Option<&str>) -> ExplanationCard {
        ExplanationCard::Word {
            source_text: "market".to_string(),
            learning_target_text: "market".to_string(),
            headword: "market".to_string(),
            part_of_speech: Some("名词 / 动词".to_string()),
            phonetic: Some("/ˈmɑːrkɪt/".to_string()),
            basic_meanings: vec!["市场".to_string(), "推广；销售".to_string()],
            context_meaning: context_meaning.map(str::to_string),
            source_sentence: None,
            source_sentence_zh: None,
            phrases: vec![PhraseItem {
                phrase: "market share".to_string(),
                meaning: "市场份额".to_string(),
            }],
            near_meanings: vec![NearMeaningItem {
                term: "promote".to_string(),
                meaning: "强调宣传和推广".to_string(),
            }],
            examples: vec![ExampleItem {
                en: "The company entered a new market.".to_string(),
                zh: "这家公司进入了一个新市场。".to_string(),
            }],
            review_hint: Some("注意名词和动词用法。".to_string()),
        }
    }

    #[test]
    fn word_card_validates() {
        assert!(validate_explanation_card(&input("market", None), &valid_word_card(None)).is_ok());
    }

    #[test]
    fn camel_case_identifier_is_classified_as_word() {
        assert_eq!(classify_query_type("anchorRect").unwrap(), QueryType::Word);
    }

    #[test]
    fn standalone_abbreviations_and_identifiers_are_classified_as_words() {
        for value in [
            "FDE", "API", "GPT", "C++", ".NET", "C#", "node.js", "F.D.E.",
        ] {
            assert_eq!(
                classify_query_type(value).unwrap(),
                QueryType::Word,
                "{value}"
            );
        }
    }

    #[test]
    fn context_sensitive_word_shapes_are_detected_without_a_vocabulary() {
        for value in [
            "XYZ",
            "RAG",
            "U.S.",
            "C++",
            "node.js",
            "anchorRect",
            "API_v2",
        ] {
            assert!(is_context_sensitive_word(value), "{value}");
        }
        for value in ["market", "running", "useful"] {
            assert!(!is_context_sensitive_word(value), "{value}");
        }
    }

    #[test]
    fn query_direction_and_english_target_are_deterministic() {
        assert_eq!(
            determine_query_direction("界面").unwrap(),
            QueryDirection::ZhToEn
        );
        assert_eq!(
            determine_query_direction("fine-tuning").unwrap(),
            QueryDirection::EnToZh
        );
        assert_eq!(
            determine_query_direction("在 Rust/generation 中").unwrap(),
            QueryDirection::EnToZh
        );
        assert_eq!(
            normalize_english_learning_target("在 Rust/generation 中").unwrap(),
            "Rust/generation"
        );
        assert_eq!(
            normalize_english_learning_target("  Memory/Review   feed  ").unwrap(),
            "Memory/Review feed"
        );
        assert!(determine_query_direction("  42 + / ! ").is_err());
    }

    #[test]
    fn mixed_query_preserves_code_identifier_boundaries() {
        let cases = [
            ("学习 C++。", "C++"),
            ("使用 .NET", ".NET"),
            ("编写 C# 程序", "C#"),
            ("运行 node.js。", "node.js"),
        ];

        for (query, expected) in cases {
            assert_eq!(normalize_english_learning_target(query).unwrap(), expected);
        }
    }

    #[test]
    fn chinese_query_types_use_local_character_boundaries() {
        assert_eq!(classify_query_type("界面").unwrap(), QueryType::Word);
        assert_eq!(
            classify_query_type("规范英文目标").unwrap(),
            QueryType::Phrase
        );
        assert_eq!(
            classify_query_type("这个结果必须保留原始中文并输出自然英文。").unwrap(),
            QueryType::Sentence
        );
        assert_eq!(
            classify_query_type("第一段说明问题。\n第二段说明解决办法。").unwrap(),
            QueryType::Paragraph
        );
    }

    #[test]
    fn chinese_learning_target_must_be_valid_english() {
        let mut card = ExplanationCard::Word {
            source_text: "界面".to_string(),
            learning_target_text: "interface".to_string(),
            headword: "interface".to_string(),
            part_of_speech: Some("noun".to_string()),
            phonetic: None,
            basic_meanings: vec!["界面".to_string()],
            context_meaning: None,
            source_sentence: None,
            source_sentence_zh: None,
            phrases: vec![],
            near_meanings: vec![],
            examples: vec![],
            review_hint: None,
        };
        assert!(validate_explanation_card(&input("界面", None), &card).is_ok());
        card.set_learning_target_text("界面".to_string());
        let errors = validate_explanation_card(&input("界面", None), &card).unwrap_err();
        assert!(errors.iter().any(|error| error.contains("有效拉丁英文")));
    }

    #[test]
    fn phrase_result_validates() {
        let card = ExplanationCard::Phrase {
            source_text: "in progress".to_string(),
            learning_target_text: "in progress".to_string(),
            basic_meaning: "正在进行中".to_string(),
            context_meaning: None,
            composition: Some("介词短语，常作表语。".to_string()),
            source_sentence: None,
            source_sentence_zh: None,
            examples: vec![],
            review_hint: None,
        };

        assert!(validate_explanation_card(&input("in progress", None), &card).is_ok());
    }

    #[test]
    fn sentence_longer_than_120_characters_validates() {
        let query = "This implementation keeps the original selection available while the asynchronous explanation request is running, so the result can still be placed beside the source sentence.";
        let card = ExplanationCard::Sentence {
            source_text: query.to_string(),
            learning_target_text: query.to_string(),
            translation: "该实现会在异步解释请求运行期间保留原始选区，因此结果仍可显示在源句旁边。"
                .to_string(),
            key_points: vec![],
            explanation: None,
            review_hint: None,
        };

        assert!(query.chars().count() > 120);
        assert!(validate_explanation_card(&input(query, None), &card).is_ok());
    }

    #[test]
    fn paragraph_validates() {
        let query = "The first sentence explains the current state. The second sentence describes the next action.";
        let card = ExplanationCard::Paragraph {
            source_text: query.to_string(),
            learning_target_text: query.to_string(),
            translation: "第一句解释当前状态。第二句描述下一步操作。".to_string(),
            key_points: vec![KeyPointItem {
                expression: "current state".to_string(),
                meaning: "当前状态".to_string(),
            }],
            summary: Some("说明当前状态和下一步。".to_string()),
        };

        assert!(validate_explanation_card(&input(query, None), &card).is_ok());
    }

    #[test]
    fn invalid_sentence_without_translation_fails() {
        let query = "This sentence has a complete grammatical structure.";
        let card = ExplanationCard::Sentence {
            source_text: query.to_string(),
            learning_target_text: query.to_string(),
            translation: String::new(),
            key_points: vec![],
            explanation: None,
            review_hint: None,
        };

        let errors = validate_explanation_card(&input(query, None), &card).unwrap_err();
        assert!(errors.iter().any(|error| error.contains("translation")));
    }

    #[test]
    fn invalid_paragraph_beyond_limit_fails() {
        let query = format!("{}\n{}", "a".repeat(2_100), "b".repeat(2_100));
        let card = ExplanationCard::Paragraph {
            source_text: query.clone(),
            learning_target_text: query.clone(),
            translation: "超长段落".to_string(),
            key_points: vec![],
            summary: None,
        };

        let errors = validate_explanation_card(&input(&query, None), &card).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.contains("4096") && error.contains("queryText")));
    }

    #[test]
    fn context_meaning_without_context_text_fails() {
        let card = valid_word_card(Some("在上下文中表示推广。"));
        let errors = validate_explanation_card(&input("market", None), &card).unwrap_err();

        assert!(errors.iter().any(|error| error.contains("contextMeaning")));
    }

    #[test]
    fn source_sentence_without_translation_is_valid() {
        let mut card = valid_word_card(None);
        if let ExplanationCard::Word {
            source_sentence,
            source_sentence_zh,
            ..
        } = &mut card
        {
            *source_sentence = Some("这段逻辑会复用 Rust generation。".to_string());
            *source_sentence_zh = None;
        }

        assert!(validate_explanation_card(
            &input("market", Some("这段逻辑会复用 Rust generation。")),
            &card,
        )
        .is_ok());
    }

    #[test]
    fn source_sentence_translation_without_source_is_invalid() {
        let mut card = valid_word_card(None);
        if let ExplanationCard::Word {
            source_sentence,
            source_sentence_zh,
            ..
        } = &mut card
        {
            *source_sentence = None;
            *source_sentence_zh = Some("这是一条没有原句的译文。".to_string());
        }

        let errors = validate_explanation_card(&input("market", None), &card).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.contains("sourceSentenceZh") && error.contains("sourceSentence")));
    }

    #[test]
    fn english_source_sentence_without_translation_is_invalid() {
        let mut card = valid_word_card(None);
        if let ExplanationCard::Word {
            source_sentence,
            source_sentence_zh,
            ..
        } = &mut card
        {
            *source_sentence = Some("The market remained open after the request finished.".into());
            *source_sentence_zh = None;
        }

        let errors = validate_explanation_card(
            &input(
                "market",
                Some("The market remained open after the request finished."),
            ),
            &card,
        )
        .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.contains("非中文主导") && error.contains("sourceSentenceZh")));
    }

    #[test]
    fn english_source_sentence_with_translation_is_valid() {
        let mut card = valid_word_card(None);
        if let ExplanationCard::Word {
            source_sentence,
            source_sentence_zh,
            ..
        } = &mut card
        {
            *source_sentence = Some("The market remained open after the request finished.".into());
            *source_sentence_zh = Some("请求结束后市场仍然开放。".into());
        }

        assert!(validate_explanation_card(
            &input(
                "market",
                Some("The market remained open after the request finished."),
            ),
            &card,
        )
        .is_ok());
    }

    #[test]
    fn chinese_dominant_source_sentence_discards_redundant_translation() {
        let mut card = valid_word_card(None);
        if let ExplanationCard::Word {
            source_sentence,
            source_sentence_zh,
            ..
        } = &mut card
        {
            *source_sentence = Some(
                "ReadRay 会在请求结束前检查 Rust generation，避免旧结果覆盖新结果。".to_string(),
            );
            *source_sentence_zh = Some("ReadRay 会检查请求代次，避免旧结果覆盖。".to_string());
        }

        normalize_source_sentence_translation(&mut card);

        assert!(is_primarily_chinese_source_sentence(
            "ReadRay 会在请求结束前检查 Rust generation，避免旧结果覆盖新结果。"
        ));
        assert!(matches!(
            card,
            ExplanationCard::Word {
                source_sentence_zh: None,
                ..
            }
        ));
        assert!(!is_primarily_chinese_source_sentence(
            "The Memory and Review pages keep the original learning record."
        ));
    }

    #[test]
    fn mixed_han_latin_source_sentence_no_longer_requires_translation() {
        assert!(is_primarily_chinese_source_sentence("GLM Coding 7天体验卡"));
        assert!(is_primarily_chinese_source_sentence("赖鑫 CLAY-bug"));

        let mut card = valid_word_card(None);
        if let ExplanationCard::Word {
            source_sentence,
            source_sentence_zh,
            ..
        } = &mut card
        {
            *source_sentence = Some("GLM Coding 7天体验卡".into());
            *source_sentence_zh = None;
        }

        assert!(
            validate_explanation_card(&input("market", Some("GLM Coding 7天体验卡")), &card,)
                .is_ok()
        );
    }
}
