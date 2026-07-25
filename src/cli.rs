use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "GitHub Timesheet")]
#[command(about = "Generate timesheets from GitHub activity", long_about = None)]
pub struct CliArgs {
    #[arg(short, long, help = "Path to timesheet template file")]
    pub file: String,

    #[arg(
        short,
        long,
        num_args = 1..,
        action = clap::ArgAction::Append,
        value_name = "REPO",
        help = "GitHub repository name(s) to merge into one timesheet"
    )]
    pub repo: Vec<String>,

    #[arg(long, help = "Configuration profile name")]
    pub profile: Option<String>,

    #[arg(
        long,
        default_value_t = 8.0,
        allow_hyphen_values = true,
        help = "Hours assigned to a workday with activity"
    )]
    pub hours_per_day: f64,

    #[arg(long, help = "Start date (YYYY-MM-DD)")]
    pub start: String,

    #[arg(long, help = "End date (YYYY-MM-DD)")]
    pub end: String,
}

impl CliArgs {
    pub fn parse_args() -> Self {
        Self::parse()
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.file.is_empty() {
            return Err(anyhow::anyhow!("Template file path is required"));
        }

        if !self.hours_per_day.is_finite() || self.hours_per_day <= 0.0 {
            return Err(anyhow::anyhow!("Hours per day must be a positive number"));
        }

        chrono::NaiveDate::parse_from_str(&self.start, "%Y-%m-%d")
            .map_err(|e| anyhow::anyhow!("Invalid start date format: {}", e))?;

        chrono::NaiveDate::parse_from_str(&self.end, "%Y-%m-%d")
            .map_err(|e| anyhow::anyhow!("Invalid end date format: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn required_args() -> Vec<&'static str> {
        vec![
            "work-logr",
            "--file",
            "template.xlsx",
            "--start",
            "2026-06-01",
            "--end",
            "2026-06-07",
        ]
    }

    #[test]
    fn parses_profile_and_custom_hours_per_day() {
        let mut args = required_args();
        args.extend(["--profile", "estate-grid", "--hours-per-day", "7.5"]);

        let parsed = CliArgs::try_parse_from(args).expect("arguments should parse");

        assert_eq!(parsed.profile.as_deref(), Some("estate-grid"));
        assert_eq!(parsed.hours_per_day, 7.5);
    }

    #[test]
    fn defaults_hours_per_day_to_eight() {
        let parsed = CliArgs::try_parse_from(required_args()).expect("arguments should parse");

        assert_eq!(parsed.hours_per_day, 8.0);
    }

    #[test]
    fn parses_multiple_repos() {
        let mut args = required_args();
        args.extend(["--repo", "repo1", "repo2", "repo3"]);

        let parsed = CliArgs::try_parse_from(args).expect("arguments should parse");

        assert_eq!(parsed.repo, vec!["repo1", "repo2", "repo3"]);
    }

    #[test]
    fn rejects_non_positive_or_non_finite_hours_per_day() {
        for hours in ["0", "-1", "NaN", "inf"] {
            let mut args = required_args();
            args.extend(["--hours-per-day", hours]);
            let parsed = CliArgs::try_parse_from(args).expect("f64 argument should parse");

            let error = parsed.validate().expect_err("invalid hours should fail");

            assert!(
                error
                    .to_string()
                    .contains("Hours per day must be a positive number"),
                "unexpected validation error for {hours}: {error}"
            );
        }
    }
}
