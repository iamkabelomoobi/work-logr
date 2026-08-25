use super::client::{GitHubClient, GitHubError};
use crate::timesheet::model::GitHubCommit;

pub async fn fetch_pr_commits(
    client: &GitHubClient,
    pr_number: u32,
) -> Result<Vec<GitHubCommit>, GitHubError> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/pulls/{}/commits",
        client.owner, client.repo, pr_number
    );

    let mut all_commits = Vec::new();
    let mut page = 1;
    let per_page = 100;

    loop {
        let per_page_str = per_page.to_string();
        let page_str = page.to_string();
        let (status, text) = client
            .get_with_retry(
                &url,
                &[
                    ("per_page", per_page_str.as_str()),
                    ("page", page_str.as_str()),
                ],
            )
            .await?;

        if !status.is_success() {
            return Err(GitHubError::RequestFailed(format!(
                "GitHub API error ({}): {}",
                status, text
            )));
        }

        if text.is_empty() {
            break;
        }

        let commits: Vec<GitHubCommit> =
            serde_json::from_str(&text).map_err(|e| GitHubError::JsonError(e.to_string()))?;

        if commits.is_empty() {
            break;
        }

        all_commits.extend(commits);
        page += 1;
    }

    Ok(all_commits)
}

pub async fn fetch_commits(
    client: &GitHubClient,
    author: &str,
    since: &str,
    until: &str,
) -> Result<Vec<GitHubCommit>, GitHubError> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/commits",
        client.owner, client.repo
    );

    let mut all_commits = Vec::new();
    let mut page = 1;
    let per_page = 100;

    loop {
        let per_page_str = per_page.to_string();
        let page_str = page.to_string();
        let (status, text) = client
            .get_with_retry(
                &url,
                &[
                    ("author", author),
                    ("since", since),
                    ("until", until),
                    ("per_page", per_page_str.as_str()),
                    ("page", page_str.as_str()),
                ],
            )
            .await?;

        if !status.is_success() {
            return Err(GitHubError::RequestFailed(format!(
                "GitHub API error ({}): {}",
                status, text
            )));
        }

        if text.is_empty() {
            break;
        }

        let commits: Vec<GitHubCommit> =
            serde_json::from_str(&text).map_err(|e| GitHubError::JsonError(e.to_string()))?;

        if commits.is_empty() {
            break;
        }

        all_commits.extend(commits);
        page += 1;
    }

    Ok(all_commits)
}
