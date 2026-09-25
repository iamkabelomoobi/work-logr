use super::error::AiError;
use super::models::{AiBatchEnrichment, AiBatchResponse, ChatCompletionResponse};
use super::prompts::SYSTEM_PROMPT;
use crate::timesheet::model::TimesheetEntry;
use reqwest::header::RETRY_AFTER;
use serde_json::json;
use std::collections::HashSet;
use std::time::Duration;

const GROQ_CHAT_COMPLETIONS_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GROQ_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const GROQ_MAX_ATTEMPTS: usize = 4;
const GROQ_RETRY_BUFFER: Duration = Duration::from_millis(250);
const GROQ_SCHEMA_RETRY_DELAY: Duration = Duration::from_millis(750);

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

    pub async fn enrich_batch(
        &self,
        entries: &[TimesheetEntry],
    ) -> Result<Vec<AiBatchEnrichment>, AiError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let activities = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                json!({
                    "index": index,
                    "type": entry.entry_type,
                    "number": entry.number,
                    "title": entry.title,
                    "status": entry.status,
                    "date": entry.date,
                    "url": entry.url,
                })
            })
            .collect::<Vec<_>>();

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
                        "Rewrite and classify every GitHub activity below. Return exactly one output entry for every input activity. Preserve each numeric index exactly and do not merge, omit, reorder semantically, or invent activities. IMPORTANT: the top-level JSON must be an object whose \"entries\" field is directly an ARRAY. Never wrap the array in an \"items\" object. Valid shape example: {\"entries\":[{\"index\":0,\"description\":\"...\",\"category\":\"Other\",\"technical_area\":\"Other\"}]}. Activities:\n{}",
                        serde_json::to_string_pretty(&activities)?
                    )
                }
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "worklog_batch_enrichment",
                    "strict": true,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "entries": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "index": {
                                            "type": "integer",
                                            "minimum": 0
                                        },
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
                                        "index",
                                        "description",
                                        "category",
                                        "technical_area"
                                    ],
                                    "additionalProperties": false
                                }
                            }
                        },
                        "required": ["entries"],
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
                return parse_batch_enrichment(&body, entries.len());
            }

            let schema_validation_failed = is_schema_validation_failure(status, &body);
            let retryable = status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error()
                || schema_validation_failed;

            if retryable && attempt < GROQ_MAX_ATTEMPTS {
                let delay = if schema_validation_failed {
                    GROQ_SCHEMA_RETRY_DELAY
                } else {
                    retry_after.unwrap_or_else(|| exponential_backoff(attempt))
                };

                eprintln!(
                    "Groq returned {}. Retrying batch in {:.2}s (attempt {}/{})...",
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

fn parse_batch_enrichment(
    body: &str,
    expected_len: usize,
) -> Result<Vec<AiBatchEnrichment>, AiError> {
    let completion: ChatCompletionResponse = serde_json::from_str(body)?;
    let content = completion
        .choices
        .into_iter()
        .next()
        .ok_or(AiError::EmptyChoices)?
        .message
        .content
        .ok_or(AiError::EmptyContent)?;

    let response = serde_json::from_str::<AiBatchResponse>(&content)?;
    validate_batch_mapping(response.entries, expected_len)
}

fn validate_batch_mapping(
    mut entries: Vec<AiBatchEnrichment>,
    expected_len: usize,
) -> Result<Vec<AiBatchEnrichment>, AiError> {
    if entries.len() != expected_len {
        return Err(AiError::InvalidBatchMapping(format!(
            "expected {expected_len} entries but received {}",
            entries.len()
        )));
    }

    let mut seen = HashSet::with_capacity(expected_len);

    for entry in &entries {
        if entry.index >= expected_len {
            return Err(AiError::InvalidBatchMapping(format!(
                "index {} is outside expected range 0..{}",
                entry.index, expected_len
            )));
        }

        if !seen.insert(entry.index) {
            return Err(AiError::InvalidBatchMapping(format!(
                "duplicate index {}",
                entry.index
            )));
        }

        if entry.description.trim().is_empty() {
            return Err(AiError::InvalidBatchMapping(format!(
                "entry {} has an empty description",
                entry.index
            )));
        }
    }

    entries.sort_by_key(|entry| entry.index);
    Ok(entries)
}

fn is_schema_validation_failure(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::BAD_REQUEST
        && (body.contains("\"code\":\"json_validate_failed\"")
            || body.contains("json_validate_failed"))
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

    fn batch_entry(index: usize) -> AiBatchEnrichment {
        AiBatchEnrichment {
            index,
            description: format!("Description {index}"),
            category: "Other".to_string(),
            technical_area: "Other".to_string(),
        }
    }

    #[test]
    fn validates_and_orders_complete_batch_mapping() {
        let mapped = validate_batch_mapping(
            vec![batch_entry(2), batch_entry(0), batch_entry(1)],
            3,
        )
        .expect("complete batch should validate");

        assert_eq!(
            mapped.iter().map(|entry| entry.index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn rejects_duplicate_batch_indexes() {
        let error = validate_batch_mapping(vec![batch_entry(0), batch_entry(0)], 2)
            .expect_err("duplicate indexes should fail");

        assert!(error.to_string().contains("duplicate index 0"));
    }

    #[test]
    fn rejects_missing_batch_entries() {
        let error = validate_batch_mapping(vec![batch_entry(0)], 2)
            .expect_err("missing entries should fail");

        assert!(error
            .to_string()
            .contains("expected 2 entries but received 1"));
    }

    #[test]
    fn identifies_groq_schema_validation_failure() {
        let body = r#"{"error":{"code":"json_validate_failed"}}"#;

        assert!(is_schema_validation_failure(
            reqwest::StatusCode::BAD_REQUEST,
            body
        ));
        assert!(!is_schema_validation_failure(
            reqwest::StatusCode::UNAUTHORIZED,
            body
        ));
    }

    #[test]
    fn does_not_retry_unrelated_bad_request() {
        let body = r#"{"error":{"code":"invalid_request_error"}}"#;

        assert!(!is_schema_validation_failure(
            reqwest::StatusCode::BAD_REQUEST,
            body
        ));
    }

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
