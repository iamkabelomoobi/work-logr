use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitHubError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(String),
    #[error("JSON parsing failed: {0}")]
    JsonError(String),
    #[error("API error")]
    RequestError(#[from] reqwest::Error),
}

pub struct GitHubClient {
    pub client: reqwest::Client,
    pub token: String,
    pub owner: String,
    pub repo: String,
}

impl GitHubClient {
    pub fn new(token: String, owner: String, repo: String) -> Self {
        let client = reqwest::Client::new();
        GitHubClient {
            client,
            token,
            owner,
            repo,
        }
    }

    pub fn auth_header(&self) -> String {
        format!("token {}", self.token)
    }
}
