use serde::{Deserialize, Serialize};

const MAX_QUERY_TEXT_LEN: usize = 120;
const MAX_CONTEXT_TEXT_LEN: usize = 2_000;
const MAX_HEADWORD_LEN: usize = 120;
const MAX_PHONETIC_LEN: usize = 80;
const MAX_BASIC_MEANING_LEN: usize = 400;
const MAX_CONTEXT_MEANING_LEN: usize = 400;
const MIN_EXAMPLES: usize = 1;
const MAX_PHRASES: usize = 3;
const MAX_NEAR_MEANINGS: usize = 3;
const MAX_EXAMPLES: usize = 2;
const MAX_PHRASE_TEXT_LEN: usize = 120;
const MAX_ITEM_MEANING_LEN: usize = 240;
const MAX_EXAMPLE_TEXT_LEN: usize = 300;
const MAX_DIFFICULTY_LEN: usize = 80;
const MAX_REVIEW_HINT_LEN: usize = 240;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryType {
    Word,
    Phrase,
    Sentence,
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
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationCard {
    pub query_type: QueryType,
    pub headword: String,
    pub phonetic: Option<String>,
    pub basic_meaning: String,
    pub context_meaning: Option<String>,
    pub phrases: Vec<PhraseItem>,
    pub near_meanings: Vec<NearMeaningItem>,
    pub examples: Vec<ExampleItem>,
    pub difficulty: Option<String>,
    pub review_hint: Option<String>,
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

pub fn validate_explanation_card(
    input: &CaptureInput,
    card: &ExplanationCard,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    validate_required_text(&mut errors, "captureInput.queryText", &input.query_text);
    validate_text_len(
        &mut errors,
        "captureInput.queryText",
        &input.query_text,
        MAX_QUERY_TEXT_LEN,
    );
    validate_optional_text_len(
        &mut errors,
        "captureInput.contextText",
        input.context_text.as_deref(),
        MAX_CONTEXT_TEXT_LEN,
    );

    validate_query_type(&mut errors, &card.query_type);
    validate_required_text(&mut errors, "explanationCard.headword", &card.headword);
    validate_text_len(
        &mut errors,
        "explanationCard.headword",
        &card.headword,
        MAX_HEADWORD_LEN,
    );
    validate_optional_text_len(
        &mut errors,
        "explanationCard.phonetic",
        card.phonetic.as_deref(),
        MAX_PHONETIC_LEN,
    );
    validate_required_text(
        &mut errors,
        "explanationCard.basicMeaning",
        &card.basic_meaning,
    );
    validate_text_len(
        &mut errors,
        "explanationCard.basicMeaning",
        &card.basic_meaning,
        MAX_BASIC_MEANING_LEN,
    );
    validate_optional_text_len(
        &mut errors,
        "explanationCard.contextMeaning",
        card.context_meaning.as_deref(),
        MAX_CONTEXT_MEANING_LEN,
    );

    let has_context = input
        .context_text
        .as_deref()
        .is_some_and(|value| !is_blank(value));
    let has_context_meaning = card
        .context_meaning
        .as_deref()
        .is_some_and(|value| !is_blank(value));
    if !has_context && has_context_meaning {
        errors.push(
            "explanationCard.contextMeaning 只有在 captureInput.contextText 存在时才允许提供。"
                .to_string(),
        );
    }

    validate_array_len(
        &mut errors,
        "explanationCard.phrases",
        card.phrases.len(),
        0,
        MAX_PHRASES,
    );
    for (index, item) in card.phrases.iter().enumerate() {
        let path = format!("explanationCard.phrases[{index}]");
        validate_required_text(&mut errors, &format!("{path}.phrase"), &item.phrase);
        validate_text_len(
            &mut errors,
            &format!("{path}.phrase"),
            &item.phrase,
            MAX_PHRASE_TEXT_LEN,
        );
        validate_required_text(&mut errors, &format!("{path}.meaning"), &item.meaning);
        validate_text_len(
            &mut errors,
            &format!("{path}.meaning"),
            &item.meaning,
            MAX_ITEM_MEANING_LEN,
        );
    }

    validate_array_len(
        &mut errors,
        "explanationCard.nearMeanings",
        card.near_meanings.len(),
        0,
        MAX_NEAR_MEANINGS,
    );
    for (index, item) in card.near_meanings.iter().enumerate() {
        let path = format!("explanationCard.nearMeanings[{index}]");
        validate_required_text(&mut errors, &format!("{path}.term"), &item.term);
        validate_text_len(
            &mut errors,
            &format!("{path}.term"),
            &item.term,
            MAX_PHRASE_TEXT_LEN,
        );
        validate_required_text(&mut errors, &format!("{path}.meaning"), &item.meaning);
        validate_text_len(
            &mut errors,
            &format!("{path}.meaning"),
            &item.meaning,
            MAX_ITEM_MEANING_LEN,
        );
    }

    validate_array_len(
        &mut errors,
        "explanationCard.examples",
        card.examples.len(),
        MIN_EXAMPLES,
        MAX_EXAMPLES,
    );
    for (index, item) in card.examples.iter().enumerate() {
        let path = format!("explanationCard.examples[{index}]");
        validate_required_text(&mut errors, &format!("{path}.en"), &item.en);
        validate_text_len(
            &mut errors,
            &format!("{path}.en"),
            &item.en,
            MAX_EXAMPLE_TEXT_LEN,
        );
        validate_required_text(&mut errors, &format!("{path}.zh"), &item.zh);
        validate_text_len(
            &mut errors,
            &format!("{path}.zh"),
            &item.zh,
            MAX_EXAMPLE_TEXT_LEN,
        );
    }

    validate_optional_text_len(
        &mut errors,
        "explanationCard.difficulty",
        card.difficulty.as_deref(),
        MAX_DIFFICULTY_LEN,
    );
    validate_optional_text_len(
        &mut errors,
        "explanationCard.reviewHint",
        card.review_hint.as_deref(),
        MAX_REVIEW_HINT_LEN,
    );

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_query_type(errors: &mut Vec<String>, query_type: &QueryType) {
    if !matches!(
        query_type,
        QueryType::Word | QueryType::Phrase | QueryType::Sentence
    ) {
        errors.push("explanationCard.queryType 必须是 word、phrase 或 sentence。".to_string());
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

    fn word_input_without_context() -> CaptureInput {
        CaptureInput {
            query_text: "market".to_string(),
            context_text: None,
            source_type: SourceType::Manual,
        }
    }

    fn word_input_with_context() -> CaptureInput {
        CaptureInput {
            query_text: "market".to_string(),
            context_text: Some("They plan to market the product in Europe.".to_string()),
            source_type: SourceType::Clipboard,
        }
    }

    fn valid_word_card() -> ExplanationCard {
        ExplanationCard {
            query_type: QueryType::Word,
            headword: "market".to_string(),
            phonetic: Some("/ˈmɑːrkɪt/".to_string()),
            basic_meaning: "市场；销售；推广".to_string(),
            context_meaning: None,
            phrases: vec![
                PhraseItem {
                    phrase: "market share".to_string(),
                    meaning: "市场份额".to_string(),
                },
                PhraseItem {
                    phrase: "target market".to_string(),
                    meaning: "目标市场".to_string(),
                },
            ],
            near_meanings: vec![
                NearMeaningItem {
                    term: "sell".to_string(),
                    meaning: "强调卖出商品或服务".to_string(),
                },
                NearMeaningItem {
                    term: "promote".to_string(),
                    meaning: "强调宣传和推广".to_string(),
                },
            ],
            examples: vec![
                ExampleItem {
                    en: "They plan to market the product in Europe.".to_string(),
                    zh: "他们计划在欧洲推广这款产品。".to_string(),
                },
                ExampleItem {
                    en: "The company entered a new market.".to_string(),
                    zh: "这家公司进入了一个新市场。".to_string(),
                },
            ],
            difficulty: Some("B1".to_string()),
            review_hint: Some("注意名词“市场”和动词“推广”的区别。".to_string()),
        }
    }

    #[test]
    fn valid_word_card_without_context_passes() {
        let input = word_input_without_context();
        let card = valid_word_card();

        assert!(validate_explanation_card(&input, &card).is_ok());
    }

    #[test]
    fn valid_word_card_with_context_meaning_and_context_text_passes() {
        let input = word_input_with_context();
        let mut card = valid_word_card();
        card.context_meaning = Some("在这句话中表示“推广、销售”。".to_string());

        assert!(validate_explanation_card(&input, &card).is_ok());
    }

    #[test]
    fn context_meaning_without_context_text_fails() {
        let input = word_input_without_context();
        let mut card = valid_word_card();
        card.context_meaning = Some("在上下文中表示推广。".to_string());

        let errors = validate_explanation_card(&input, &card).unwrap_err();

        assert!(errors.iter().any(|error| error.contains("contextMeaning")));
    }

    #[test]
    fn missing_example_zh_fails() {
        let input = word_input_without_context();
        let mut card = valid_word_card();
        card.examples[0].zh.clear();

        let errors = validate_explanation_card(&input, &card).unwrap_err();

        assert!(errors.iter().any(|error| error.contains("examples[0].zh")));
    }

    #[test]
    fn empty_examples_fails() {
        let input = word_input_without_context();
        let mut card = valid_word_card();
        card.examples.clear();

        let errors = validate_explanation_card(&input, &card).unwrap_err();

        assert!(errors.iter().any(|error| error.contains("examples")));
    }

    #[test]
    fn too_many_phrases_fails() {
        let input = word_input_without_context();
        let mut card = valid_word_card();
        card.phrases.push(PhraseItem {
            phrase: "mass market".to_string(),
            meaning: "大众市场".to_string(),
        });
        card.phrases.push(PhraseItem {
            phrase: "market leader".to_string(),
            meaning: "市场领导者".to_string(),
        });

        let errors = validate_explanation_card(&input, &card).unwrap_err();

        assert!(errors.iter().any(|error| error.contains("phrases")));
    }

    #[test]
    fn empty_headword_and_basic_meaning_fail() {
        let input = word_input_without_context();
        let mut card = valid_word_card();
        card.headword.clear();
        card.basic_meaning = "   ".to_string();

        let errors = validate_explanation_card(&input, &card).unwrap_err();

        assert!(errors.iter().any(|error| error.contains("headword")));
        assert!(errors.iter().any(|error| error.contains("basicMeaning")));
    }
}
