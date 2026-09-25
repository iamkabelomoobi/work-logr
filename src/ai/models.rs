use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiBatchEnrichment {
    pub index: usize,
    pub description: String,
    pub category: String,
    pub technical_area: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiBatchResponse {
    pub entries: Vec<AiBatchEnrichment>,
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
    fn parses_batch_structured_enrichment() {
        let parsed: AiBatchResponse = serde_json::from_str(
            r#"{
                "entries": [
                    {
                        "index": 0,
                        "description": "Corrected Funding Status filtering.",
                        "category": "Bug Fix",
                        "technical_area": "Backend"
                    },
                    {
                        "index": 1,
                        "description": "Updated the rental payment filters.",
                        "category": "Bug Fix",
                        "technical_area": "Frontend"
                    }
                ]
            }"#,
        )
        .expect("valid batch output should parse");

        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].index, 0);
        assert_eq!(parsed.entries[1].index, 1);
    }
}
