use std::env;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub github_token: String,
    pub github_owner: String,
    pub github_repo: String,
    pub github_user: String,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    MissingVar(String),
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let github_token = env::var("GITHUB_TOKEN")
            .map_err(|_| ConfigError::MissingVar("GITHUB_TOKEN".to_string()))?;
        let github_owner = env::var("GITHUB_OWNER")
            .map_err(|_| ConfigError::MissingVar("GITHUB_OWNER".to_string()))?;
        let github_repo = env::var("GITHUB_REPO")
            .map_err(|_| ConfigError::MissingVar("GITHUB_REPO".to_string()))?;
        let github_user = env::var("GITHUB_USER")
            .map_err(|_| ConfigError::MissingVar("GITHUB_USER".to_string()))?;

        Ok(Config {
            github_token,
            github_owner,
            github_repo,
            github_user,
        })
    }
}
