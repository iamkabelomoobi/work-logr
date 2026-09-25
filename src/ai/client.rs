use super::error::AiError;
use super::models::{AiEnrichment, ChatCompletionResponse};
use super::prompts::SYSTEM_PROMPT;
use crate::timesheet::model::TimesheetEntry;
use serde_json::json;
use std::time::Duration;

const GROQ_CHAT_COMPLETIONS_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GROQ_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct GroqClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl GroqClient {
    pub fn from_env(model: impl Into<String>) -> Result<Self, AiError> {
        let api_key = std::env::var("GROQ_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(AiError::MissingApiKey)?;

        let client = reqwest::Client::builder()
            .timeout(GROQ_REQUEST_TIMEOUT)
            .build()?;

        Ok(Self {
            client,
            api_key,
            model: model.into(),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub async fn enrich(&self, entry: &TimesheetEntry) -> Result<AiEnrichment, AiError> {
        let activity = json!({
            "type": entry.entry_type,
            "number": entry.number,
            "title": entry.title,
            "status": entry.status,
            "date": entry.date,
            "url": entry.url,
        });

        let request_body = json!({
            "model": self.model,
            "temperature": 0.2,
            "messages": [
                {
                    "role": "system",
                    "content": SYSTEM_PROMPT
                },
                {
                    "role": "user",
                    "content": format!(
                        "Rewrite this GitHub activity into one factual timesheet description and classify it. Activity:\n{}",
                        serde_json::to_string_pretty(&activity)?
                    )
                }
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "worklog_enrichment",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "description": {
                                "type": "string"
                            },
                            "category": {
                                "type": "string",
                                "enum": [
                                    "Feature",
                                    "Bug Fix",
                                    "Testing",
                                    "Investigation",
                                    "Refactor",
                                    "Documentation",
                                    "Maintenance",
                                    "Other"
                                ]
                            },
                            "technical_area": {
                                "type": "string",
                                "enum": [
                                    "Frontend",
                                    "Backend",
                                    "Database",
                                    "DevOps",
                                    "Testing",
                                    "Full Stack",
                                    "Other"
                                ]
                            }
                        },
                        "required": [
                            "description",
                            "category",
                            "technical_area"
                        ],
                        "additionalProperties": false
                    }
                }
            }
        });

        let response = self
            .client
            .post(GROQ_CHAT_COMPLETIONS_URL)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            return Err(AiError::Api { status, body });
        }

        let completion: ChatCompletionResponse = serde_json::from_str(&body)?;
        let content = completion
            .choices
            .into_iter()
            .next()
            .ok_or(AiError::EmptyChoices)?
            .message
            .content
            .ok_or(AiError::EmptyContent)?;

        let enrichment = serde_json::from_str::<AiEnrichment>(&content)?;

        if enrichment.description.trim().is_empty() {
            return Err(AiError::EmptyContent);
        }

        Ok(enrichment)
    }
}
