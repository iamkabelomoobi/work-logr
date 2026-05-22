use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimesheetEntry {
    pub entry_type: String,
    pub number: String,
    pub title: String,
    pub status: String,
    pub closed_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub assignees: String,
    pub author: String,
    pub url: String,
    pub date: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubIssue {
    pub number: i32,
    pub title: String,
    pub state: String,
    pub pull_request: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub assignees: Vec<GitHubUser>,
    pub user: GitHubUser,
    pub html_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GitHubUser {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct GitHubCommit {
    pub commit: CommitDetails,
    pub html_url: String,
    pub author: Option<GitHubUser>,
    #[serde(default)]
    pub parents: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CommitDetails {
    pub message: String,
    pub author: CommitAuthor,
    pub committer: Option<CommitAuthor>,
}

#[derive(Debug, Deserialize)]
pub struct CommitAuthor {
    pub date: String,
}
