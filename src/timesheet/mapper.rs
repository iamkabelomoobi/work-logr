use super::model::{GitHubCommit, GitHubIssue, TimesheetEntry};
use crate::utils::dates;
use chrono::Utc;
use std::collections::HashSet;

pub fn map_issues_to_entries(
    issues: Vec<GitHubIssue>,
    target_user: &str,
    start: chrono::DateTime<Utc>,
    end: chrono::DateTime<Utc>,
) -> Vec<TimesheetEntry> {
    issues
        .into_iter()
        .filter(|issue| {
            let assigned = issue.assignees.iter().any(|a| a.login == target_user);
            let created_by_user = issue.user.login == target_user;
            let touched_in_range = dates::is_in_range(Some(&issue.created_at), start, end)
                || dates::is_in_range(Some(&issue.updated_at), start, end)
                || dates::is_in_range(issue.closed_at.as_deref(), start, end);

            touched_in_range && (assigned || created_by_user)
        })
        .map(|issue| {
            let date = issue
                .closed_at
                .clone()
                .unwrap_or_else(|| issue.updated_at.clone());

            TimesheetEntry {
                entry_type: if issue.pull_request.is_some() {
                    "Pull Request".to_string()
                } else {
                    "Issue".to_string()
                },
                number: issue.number.to_string(),
                title: issue.title,
                status: issue.state,
                closed_at: issue.closed_at.unwrap_or_default(),
                created_at: issue.created_at,
                updated_at: issue.updated_at,
                assignees: issue
                    .assignees
                    .into_iter()
                    .map(|a| a.login)
                    .collect::<Vec<_>>()
                    .join(", "),
                author: issue.user.login,
                url: issue.html_url,
                date,
            }
        })
        .collect()
}

pub fn map_commits_to_entries(commits: Vec<GitHubCommit>, user: &str) -> Vec<TimesheetEntry> {
    commits
        .into_iter()
        .filter(|commit| is_user_commit(commit, user))
        .filter(|commit| !is_merge_commit(commit))
        .map(|commit| {
            let first_line = commit.commit.message.lines().next().unwrap_or("");
            let date = commit
                .commit
                .committer
                .as_ref()
                .map(|committer| committer.date.clone())
                .unwrap_or_else(|| commit.commit.author.date.clone());

            TimesheetEntry {
                entry_type: "Commit".to_string(),
                number: String::new(),
                title: first_line.to_string(),
                status: "committed".to_string(),
                closed_at: String::new(),
                created_at: commit.commit.author.date,
                updated_at: date.clone(),
                assignees: user.to_string(),
                author: user.to_string(),
                url: commit.html_url,
                date,
            }
        })
        .collect()
}

pub fn exclude_pr_linked_commits(
    commits: Vec<GitHubCommit>,
    linked_commit_urls: &HashSet<String>,
) -> Vec<GitHubCommit> {
    commits
        .into_iter()
        .filter(|commit| !linked_commit_urls.contains(&commit.html_url))
        .collect()
}

fn is_user_commit(commit: &GitHubCommit, user: &str) -> bool {
    commit
        .author
        .as_ref()
        .map(|author| author.login == user)
        .unwrap_or(false)
}

fn is_merge_commit(commit: &GitHubCommit) -> bool {
    if commit.parents.len() > 1 {
        return true;
    }

    let first_line = commit
        .commit
        .message
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();

    first_line.starts_with("merge ")
        || first_line.starts_with("merged ")
        || first_line.starts_with("merge:")
}

pub fn deduplicate_entries(entries: Vec<TimesheetEntry>) -> Vec<TimesheetEntry> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|entry| {
            let key = entry.url.clone();
            if seen.contains(&key) {
                return false;
            }
            seen.insert(key);
            true
        })
        .collect()
}

pub fn build_task_description(entry: &TimesheetEntry) -> String {
    match entry.entry_type.as_str() {
        "PR Commit" => format!("PR #{} Commit: {}", entry.number, entry.title),
        "Commit" => format!("Commit: {}", entry.title),
        _ => format!("{} #{}: {}", entry.entry_type, entry.number, entry.title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timesheet::model::{CommitAuthor, CommitDetails, GitHubCommit, GitHubUser};
    use std::collections::HashSet;

    #[test]
    fn excludes_commits_linked_to_included_pull_requests() {
        let standalone = commit(
            "Standalone fix",
            "iamkabelomoobi",
            "2026-05-18T09:00:00Z",
            None,
            1,
        );
        let linked = commit(
            "PR implementation",
            "iamkabelomoobi",
            "2026-05-18T10:00:00Z",
            None,
            1,
        );
        let linked_urls = HashSet::from([linked.html_url.clone()]);

        let filtered = exclude_pr_linked_commits(vec![standalone, linked], &linked_urls);

        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].html_url.contains("Standalone fix"));
    }

    #[test]
    fn maps_user_commits_to_committer_date() {
        let entries = map_commits_to_entries(
            vec![commit(
                "Implement Excel rows",
                "iamkabelomoobi",
                "2026-05-18T09:00:00Z",
                Some("2026-05-19T14:00:00Z"),
                1,
            )],
            "iamkabelomoobi",
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Implement Excel rows");
        assert_eq!(entries[0].date, "2026-05-19T14:00:00Z");
    }

    #[test]
    fn ignores_merge_commits() {
        let entries = map_commits_to_entries(
            vec![
                commit(
                    "Merge branch 'main' into feature",
                    "iamkabelomoobi",
                    "2026-05-18T09:00:00Z",
                    Some("2026-05-18T10:00:00Z"),
                    2,
                ),
                commit(
                    "Merged feature work into develop",
                    "iamkabelomoobi",
                    "2026-05-18T11:00:00Z",
                    Some("2026-05-18T12:00:00Z"),
                    1,
                ),
                commit(
                    "Add timesheet task grouping",
                    "iamkabelomoobi",
                    "2026-05-18T13:00:00Z",
                    Some("2026-05-18T14:00:00Z"),
                    1,
                ),
            ],
            "iamkabelomoobi",
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Add timesheet task grouping");
    }

    fn commit(
        message: &str,
        author: &str,
        author_date: &str,
        committer_date: Option<&str>,
        parent_count: usize,
    ) -> GitHubCommit {
        GitHubCommit {
            commit: CommitDetails {
                message: message.to_string(),
                author: CommitAuthor {
                    date: author_date.to_string(),
                },
                committer: committer_date.map(|date| CommitAuthor {
                    date: date.to_string(),
                }),
            },
            html_url: format!("https://github.com/example/repo/commit/{message}"),
            author: Some(GitHubUser {
                login: author.to_string(),
            }),
            parents: vec![serde_json::Value::Null; parent_count],
        }
    }
}
