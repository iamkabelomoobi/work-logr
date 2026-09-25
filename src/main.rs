mod ai;
mod cli;
mod config;
mod github;
mod timesheet;
mod utils;

use cli::CliArgs;
use config::Config;
use github::GitHubClient;
use std::path::PathBuf;
use timesheet::mapper::{deduplicate_entries, prefer_pull_requests_on_same_day};
use timesheet::weeks::split_entries_by_week;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let disable_banner = std::env::var_os("WORKLOGR_NO_BANNER").is_some();
    let stdout_is_tty = atty::is(atty::Stream::Stdout);

    if !disable_banner && stdout_is_tty {
        print_banner();
    }

    let args = CliArgs::parse_args();
    args.validate()?;

    let config = Config::from_env(args.profile.as_deref())?;
    let repos = if args.repo.is_empty() {
        vec![config.github_repo.clone()]
    } else {
        args.repo.clone()
    };

    let start_dt = chrono::DateTime::parse_from_rfc3339(&format!("{}T00:00:00Z", args.start))?
        .with_timezone(&chrono::Utc);
    let end_dt = chrono::DateTime::parse_from_rfc3339(&format!("{}T23:59:59Z", args.end))?
        .with_timezone(&chrono::Utc);

    println!("Fetching repository activity...");
    let since = format!("{}T00:00:00Z", args.start);
    let until = format!("{}T23:59:59Z", args.end);
    let mut entries = Vec::new();
    let mut repo_labels = Vec::new();

    for (idx, repo) in repos.iter().enumerate() {
        println!("Fetching repository {}/{}: {}", idx + 1, repos.len(), repo);
        let client = GitHubClient::new(
            config.github_token.clone(),
            config.github_owner.clone(),
            repo.to_string(),
        );

        println!("Fetching issues for {}...", repo);
        let issues = github::issues::fetch_issues(&client, &args.start).await?;
        println!("Fetching pull requests for {}...", repo);
        let prs = github::prs::fetch_prs(&client, &args.start).await?;
        let commits =
            github::commits::fetch_commits(&client, &config.github_user, &since, &until).await?;

        let mut repo_entries =
            timesheet::mapper::map_issues_to_entries(issues, &config.github_user, start_dt, end_dt);

        let mut pr_entries =
            timesheet::mapper::map_issues_to_entries(prs, &config.github_user, start_dt, end_dt);
        repo_entries.append(&mut pr_entries);

        let mut commit_entries =
            timesheet::mapper::map_commits_to_entries(commits, &config.github_user);
        repo_entries.append(&mut commit_entries);
        repo_entries = prefer_pull_requests_on_same_day(repo_entries);

        entries.append(&mut repo_entries);
        repo_labels.push(repo.clone());
    }

    entries = deduplicate_entries(entries);
    entries.sort_by(|a, b| a.date.cmp(&b.date));

    if args.ai {
        let groq = ai::GroqClient::from_env(args.ai_model.clone())?;
        println!(
            "Enriching {} worklog entr{} with Groq model {}...",
            entries.len(),
            if entries.len() == 1 { "y" } else { "ies" },
            groq.model()
        );

        let stats = ai::enrich_entries(&groq, &mut entries).await;
        println!(
            "AI enrichment complete: {} enriched, {} fallback",
            stats.enriched, stats.failed
        );
    }

    let weeks = utils::dates::get_week_ranges(
        chrono::NaiveDate::parse_from_str(&args.start, "%Y-%m-%d")?,
        chrono::NaiveDate::parse_from_str(&args.end, "%Y-%m-%d")?,
    );

    let output_dir = PathBuf::from("output");
    std::fs::create_dir_all(&output_dir)?;

    let entries_by_week = split_entries_by_week(entries.clone(), &weeks);
    let template_path = PathBuf::from(&args.file);
    let repo_label = repo_display_label(&repo_labels);

    for (idx, week) in weeks.iter().enumerate() {
        let filename = format!(
            "{}_{}_{}_{}_{}_to_{}.xlsx",
            repo_filename_label(&repo_labels),
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
            &repo_label,
            &config.github_user,
            week,
            args.hours_per_day,
        )?;

        println!("Generated: {}", filename);
    }

    println!("
Total entries: {}", entries.len());
    Ok(())
}

fn print_banner() {
    const CYAN: &str = "[96m";
    const YELLOW: &str = "[93m";
    const GREEN: &str = "[92m";
    const DIM: &str = "[2m";
    const BOLD: &str = "[1m";
    const RESET: &str = "[0m";

    let ascii_art = [
        r"_    _    ___   _ __  _         _        ___    __ _  _ __ ",
        r"| |  | | / _ | '__| | | __    | |      / _   / _` | '__|",
        r"| |/\| || | | || |   | |/ /    | |     | | | || (_| || |   ",
        r"\  /\  /| |_| ||_|   |   <     | |___  | |_| | \__, ||_|   ",
        r" \/  \/  \___/       |_|\_\    |_____|  \___/      | |     ",
        r"                                                    | |     ",
        r"                                                    |_|     ",
    ];

    let term_width: usize = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let art_width = ascii_art.iter().map(|l| l.len()).max().unwrap_or(60);
    let art_pad = " ".repeat(term_width.saturating_sub(art_width) / 2);

    println!();
    for line in &ascii_art {
        println!("{art_pad}{BOLD}{CYAN}{line}{RESET}");
    }
    println!();

    let print_centered = |plain: &str, style: &dyn Fn(&str) -> String| {
        let colored = style(plain);
        let pad = " ".repeat(term_width.saturating_sub(plain.len()) / 2);
        println!("{pad}{colored}");
    };

    let version = env!("CARGO_PKG_VERSION");
    let version_text = format!("Work Logr v{version}");
    print_centered(&version_text, &|t| format!("{}{}{}{}", BOLD, YELLOW, t, RESET));

    print_centered("Author: Kabelo Moobi", &|t| format!("{}{}{}", GREEN, t, RESET));

    print_centered(
        "Generate weekly Excel timesheets from GitHub activity",
        &|t| format!("{}{}{}", DIM, t, RESET),
    );
    println!();
}

fn repo_display_label(repos: &[String]) -> String {
    repos.join(", ")
}

fn repo_filename_label(repos: &[String]) -> String {
    repos
        .iter()
        .map(|repo| repo.replace('/', "-"))
        .collect::<Vec<_>>()
        .join("_")
        .to_uppercase()
}
