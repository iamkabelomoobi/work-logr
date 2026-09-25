use super::error::AiError;
use super::models::{AiEnrichment, ChatCompletionResponse};
use super::prompts::SYSTEM_PROMPT;
use crate::timesheet::model::TimesheetEntry;
use reqwest::header::RETRY_AFTER;
use serde_json::json;
use std::time::Duration;

const GROQ_CHAT_COMPLETIONS_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GROQ_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const GROQ_MAX_ATTEMPTS: usize = 4;
const GROQ_RETRY_BUFFER: Duration = Duration::from_millis(250);

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

        for attempt in 1..=GROQ_MAX_ATTEMPTS {
            let response = self
                .client
                .post(GROQ_CHAT_COMPLETIONS_URL)
                .bearer_auth(&self.api_key)
                .json(&request_body)
                .send()
                .await?;

            let status = response.status();
            let retry_after = retry_after_duration(response.headers());
            let body = response.text().await?;

            if status.is_success() {
                return parse_enrichment(&body);
            }

            let retryable = status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error();

            if retryable && attempt < GROQ_MAX_ATTEMPTS {
                let delay = retry_after.unwrap_or_else(|| exponential_backoff(attempt));
                eprintln!(
                    "Groq returned {}. Retrying in {:.2}s (attempt {}/{})...",
                    status,
                    delay.as_secs_f64(),
                    attempt + 1,
                    GROQ_MAX_ATTEMPTS
                );
                tokio::time::sleep(delay).await;
                continue;
            }

            return Err(AiError::Api { status, body });
        }

        unreachable!("Groq retry loop always returns on success or final failure")
    }
}

fn parse_enrichment(body: &str) -> Result<AiEnrichment, AiError> {
    let completion: ChatCompletionResponse = serde_json::from_str(body)?;
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

fn retry_after_duration(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let seconds = headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()?;

    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }

    Some(Duration::from_secs_f64(seconds) + GROQ_RETRY_BUFFER)
}

fn exponential_backoff(attempt: usize) -> Duration {
    let seconds = 2_u64.saturating_pow(attempt.saturating_sub(1) as u32);
    Duration::from_secs(seconds) + GROQ_RETRY_BUFFER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fractional_retry_after_header_with_buffer() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(RETRY_AFTER, "1.5".parse().expect("valid header"));

        let delay = retry_after_duration(&headers).expect("retry-after should parse");

        assert_eq!(delay, Duration::from_millis(1750));
    }

    #[test]
    fn ignores_invalid_retry_after_header() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(RETRY_AFTER, "not-a-number".parse().expect("valid header"));

        assert!(retry_after_duration(&headers).is_none());
    }

    #[test]
    fn exponential_backoff_grows_between_attempts() {
        assert_eq!(exponential_backoff(1), Duration::from_millis(1250));
        assert_eq!(exponential_backoff(2), Duration::from_millis(2250));
        assert_eq!(exponential_backoff(3), Duration::from_millis(4250));
    }
}
