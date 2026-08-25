use super::client::{GitHubClient, GitHubError};
use crate::timesheet::model::GitHubIssue;

pub async fn fetch_prs(
    client: &GitHubClient,
    since: &str,
) -> Result<Vec<GitHubIssue>, GitHubError> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/pulls",
        client.owner, client.repo
    );

    let mut all_prs = Vec::new();
    let mut page = 1;
    let per_page = 100;

    loop {
        let per_page_str = per_page.to_string();
        let page_str = page.to_string();
        let (status, text) = client
            .get_with_retry(
                &url,
                &[
                    ("state", "all"),
                    ("since", since),
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

        let prs: Vec<GitHubIssue> =
            serde_json::from_str(&text).map_err(|e| GitHubError::JsonError(e.to_string()))?;

        if prs.is_empty() {
            break;
        }

        all_prs.extend(prs);
        page += 1;
    }

    Ok(all_prs)
}
