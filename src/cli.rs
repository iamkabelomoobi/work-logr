use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "GitHub Timesheet")]
#[command(about = "Generate timesheets from GitHub activity", long_about = None)]
pub struct CliArgs {
    #[arg(short, long, help = "Path to timesheet template file")]
    pub file: String,

    #[arg(short, long, help = "GitHub repository name")]
    pub repo: Option<String>,

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

        chrono::NaiveDate::parse_from_str(&self.start, "%Y-%m-%d")
            .map_err(|e| anyhow::anyhow!("Invalid start date format: {}", e))?;

        chrono::NaiveDate::parse_from_str(&self.end, "%Y-%m-%d")
            .map_err(|e| anyhow::anyhow!("Invalid end date format: {}", e))?;

        Ok(())
    }
}
