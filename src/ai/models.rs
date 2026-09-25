use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiEnrichment {
    pub description: String,
    pub category: String,
    pub technical_area: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EnrichmentStats {
    pub enriched: usize,
    pub failed: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionResponse {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Choice {
    pub message: AssistantMessage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AssistantMessage {
    pub content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_enrichment() {
        let parsed: AiEnrichment = serde_json::from_str(
            r#"{
                "description": "Corrected Funding Status filtering.",
                "category": "Bug Fix",
                "technical_area": "Backend"
            }"#,
        )
        .expect("valid structured output should parse");

        assert_eq!(
            parsed,
            AiEnrichment {
                description: "Corrected Funding Status filtering.".to_string(),
                category: "Bug Fix".to_string(),
                technical_area: "Backend".to_string(),
            }
        );
    }

    #[test]
    fn rejects_incomplete_structured_enrichment() {
        let result = serde_json::from_str::<AiEnrichment>(
            r#"{"description":"Corrected Funding Status filtering."}"#,
        );

        assert!(result.is_err());
    }
}
