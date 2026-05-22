use super::client::{GitHubClient, GitHubError};
use crate::timesheet::model::GitHubIssue;

pub async fn fetch_issues(
    client: &GitHubClient,
    since: &str,
) -> Result<Vec<GitHubIssue>, GitHubError> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/issues",
        client.owner, client.repo
    );

    let mut all_items = Vec::new();
    let mut page = 1;
    let per_page = 100;

    loop {
        let response = client
            .client
            .get(&url)
            .header("User-Agent", "work-logr")
            .header("Authorization", client.auth_header())
            .header("Accept", "application/vnd.github.v3+json")
            .query(&[
                ("state", "all"),
                ("since", since),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ])
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            return Err(GitHubError::RequestFailed(format!(
                "GitHub API error ({}): {}",
                status, text
            )));
        }

        if text.is_empty() {
            break;
        }

        let items: Vec<GitHubIssue> =
            serde_json::from_str(&text).map_err(|e| GitHubError::JsonError(e.to_string()))?;

        if items.is_empty() {
            break;
        }

        all_items.extend(items);
        page += 1;
    }

    Ok(all_items)
}
