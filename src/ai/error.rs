use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("AI enrichment requested but GROQ_API_KEY is not configured")]
    MissingApiKey,

    #[error("Groq request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Groq API returned {status}: {body}")]
    Api { status: reqwest::StatusCode, body: String },

    #[error("Groq returned no completion choices")]
    EmptyChoices,

    #[error("Groq returned an empty completion")]
    EmptyContent,

    #[error("Groq returned invalid structured output: {0}")]
    InvalidOutput(#[from] serde_json::Error),
}
