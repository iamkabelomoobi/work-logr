mod cli;
mod config;
mod github;
mod timesheet;
mod utils;

use cli::CliArgs;
use config::Config;
use github::GitHubClient;
use std::path::PathBuf;
use timesheet::mapper::deduplicate_entries;
use timesheet::model::GitHubIssue;
use timesheet::weeks::split_entries_by_week;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let args = CliArgs::parse_args();
    args.validate()?;

    let config = Config::from_env()?;

    let repo = args.repo.as_deref().unwrap_or(&config.github_repo);

    let client = GitHubClient::new(
        config.github_token.clone(),
        config.github_owner.clone(),
        repo.to_string(),
    );

    println!("Fetching issues...");
    let issues = github::issues::fetch_issues(&client, &args.start).await?;

    println!("Fetching pull requests...");
    let prs = github::prs::fetch_prs(&client, &args.start).await?;

    let start_dt = chrono::DateTime::parse_from_rfc3339(&format!("{}T00:00:00Z", args.start))?
        .with_timezone(&chrono::Utc);
    let end_dt = chrono::DateTime::parse_from_rfc3339(&format!("{}T23:59:59Z", args.end))?
        .with_timezone(&chrono::Utc);

    println!("Fetching commits...");
    let since = format!("{}T00:00:00Z", args.start);
    let until = format!("{}T23:59:59Z", args.end);
    let mut commits =
        github::commits::fetch_commits(&client, &config.github_user, &since, &until).await?;

    let prs_for_commit_fetch: Vec<_> = prs
        .iter()
        .filter(|pr| is_pr_relevant_for_commit_fetch(pr, &config.github_user, start_dt, end_dt))
        .collect();

    println!(
        "Fetching pull request commits for {} relevant PR(s)...",
        prs_for_commit_fetch.len()
    );
    for (idx, pr) in prs_for_commit_fetch.iter().enumerate() {
        println!(
            "Fetching PR commits {}/{}: #{}",
            idx + 1,
            prs_for_commit_fetch.len(),
            pr.number
        );
        let mut pr_commits = github::commits::fetch_pr_commits(&client, pr.number as u32).await?;
        commits.append(&mut pr_commits);
    }

    let mut entries =
        timesheet::mapper::map_issues_to_entries(issues, &config.github_user, start_dt, end_dt);

    let mut pr_entries =
        timesheet::mapper::map_issues_to_entries(prs, &config.github_user, start_dt, end_dt);
    entries.append(&mut pr_entries);

    let mut commit_entries =
        timesheet::mapper::map_commits_to_entries(commits, &config.github_user);
    entries.append(&mut commit_entries);

    entries = deduplicate_entries(entries);
    entries.sort_by(|a, b| a.date.cmp(&b.date));

    let weeks = utils::dates::get_week_ranges(
        chrono::NaiveDate::parse_from_str(&args.start, "%Y-%m-%d")?,
        chrono::NaiveDate::parse_from_str(&args.end, "%Y-%m-%d")?,
    );

    let output_dir = PathBuf::from("output");
    std::fs::create_dir_all(&output_dir)?;

    let entries_by_week = split_entries_by_week(entries.clone(), &weeks);
    let template_path = PathBuf::from(&args.file);

    for (idx, week) in weeks.iter().enumerate() {
        let filename = format!(
            "{}_{}_{}_{}_{}_to_{}.xlsx",
            repo.to_uppercase(),
            config.github_user,
            "Week",
            idx + 1,
            week.start.format("%Y-%m-%d"),
            week.end.format("%Y-%m-%d")
        );

        let output_path = output_dir.join(&filename);
        timesheet::excel::export_to_excel(
            &entries_by_week[idx],
            &template_path,
            &output_path,
            repo,
            &config.github_user,
            week,
        )?;

        println!("Generated: {}", filename);
    }

    println!("\nTotal entries: {}", entries.len());
    Ok(())
}

fn is_pr_relevant_for_commit_fetch(
    pr: &GitHubIssue,
    target_user: &str,
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
) -> bool {
    let assigned = pr
        .assignees
        .iter()
        .any(|assignee| assignee.login == target_user);
    let created_by_user = pr.user.login == target_user;
    let touched_in_range = utils::dates::is_in_range(Some(&pr.created_at), start, end)
        || utils::dates::is_in_range(Some(&pr.updated_at), start, end)
        || utils::dates::is_in_range(pr.closed_at.as_deref(), start, end);

    touched_in_range && (assigned || created_by_user)
}
