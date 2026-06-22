<img width="768" height="293" alt="image" src="https://github.com/user-attachments/assets/f2cc144d-83ab-4285-a804-cc08c9ef0856" />


A Rust CLI that generates weekly Excel timesheets from GitHub issues, pull requests, and commits.

## Overview

The application fetches GitHub activity for a configured user and writes one populated `.xlsx` workbook per week using the supplied timesheet template.

## Features

- Fetches GitHub issues and pull requests for a specific user within a date range
- Fetches commits authored by the configured GitHub user
- Uses commits attached to included pull requests to suppress duplicate commit rows
- Filters items by:
  - Assignment to the target user
  - Creation by the target user
  - Date range (created, updated, or closed within range)
- Places commits on the day they were committed/pushed in GitHub commit metadata
- Writes each same-day commit or pull request to a separate Excel row with the date repeated
- Ignores merge commits, including commits with multiple parents and messages starting with `Merge`, `Merged`, or `Merge:`
- Exports filtered records into Excel workbooks using a provided `.xlsx` template
- Supports pagination for large result sets

## Project Structure

```
src/
├── main.rs              # Application entry point and orchestration
├── config.rs            # Configuration loading from environment variables
├── cli.rs               # Command-line argument parsing
├── github/
│   ├── client.rs        # GitHub API client
│   ├── issues.rs        # Issue fetching
│   ├── prs.rs           # Pull request fetching
│   └── commits.rs       # Repository and PR commit fetching
├── timesheet/
│   ├── model.rs         # GitHub and timesheet data structures
│   ├── mapper.rs        # GitHub-to-timesheet mapping and filtering
│   ├── weeks.rs         # Weekly grouping
│   └── excel.rs         # Template-based Excel generation
└── utils/dates.rs       # Date parsing and week ranges
```

## Building

```bash
cargo build --release
```

## Running

### Prerequisites

Set up environment variables in a `.env` file:

```env
GITHUB_TOKEN=your_github_token_here
GITHUB_OWNER=repository_owner
GITHUB_REPO=repository_name
GITHUB_USER=your_github_username
```

### Execute

```bash
cargo run --release -- --file templates/TimesheetTemplate.xlsx --start 2026-04-26 --end 2026-05-18 --hours-per-day 7.5
```

The application will generate one Excel workbook per week in `output/`, for example:
`REPO_iamkabelomoobi_Week_1_2026-04-26_to_2026-04-26.xlsx`

### CLI Options

| Option | Description | Default |
| --- | --- | --- |
| `--file <path>` | Path to the timesheet template file | Required |
| `--repo <name>` | Override the configured GitHub repository | `GITHUB_REPO` from the selected configuration |
| `--profile <name>` | Load profile-scoped `WORKLOGR_<PROFILE>_GITHUB_*` variables | Bare `GITHUB_*` variables |
| `--hours-per-day <f64>` | Hours assigned to a workday with activity | `8.0` |
| `--start <YYYY-MM-DD>` | Start date | Required |
| `--end <YYYY-MM-DD>` | End date | Required |

### Profiles

Profiles let multiple project configurations coexist in the same `.env` file. Profile names are
uppercased and non-alphanumeric characters are replaced with `_`, so `estate-grid` uses the
`WORKLOGR_ESTATE_GRID_*` prefix.

```env
WORKLOGR_NSFAS_GITHUB_TOKEN=your_nsfas_token
WORKLOGR_NSFAS_GITHUB_OWNER=nsfas_owner
WORKLOGR_NSFAS_GITHUB_REPO=nsfas_repo
WORKLOGR_NSFAS_GITHUB_USER=your_github_username

WORKLOGR_ESTATE_GRID_GITHUB_TOKEN=your_estate_grid_token
WORKLOGR_ESTATE_GRID_GITHUB_OWNER=estate_grid_owner
WORKLOGR_ESTATE_GRID_GITHUB_REPO=estate_grid_repo
WORKLOGR_ESTATE_GRID_GITHUB_USER=your_github_username
```

Select a profile with `--profile`:

```bash
cargo run --release -- --profile nsfas --file templates/TimesheetTemplate.xlsx --start 2026-04-26 --end 2026-05-18
cargo run --release -- --profile estate-grid --file templates/TimesheetTemplate.xlsx --start 2026-04-26 --end 2026-05-18
```

When `--profile` is omitted, the existing bare `GITHUB_TOKEN`, `GITHUB_OWNER`, `GITHUB_REPO`, and
`GITHUB_USER` variables are used.

## Generation Rules

- Each workbook covers one generated week range.
- The workbook preserves the provided template and updates the header fields.
- Rows are populated by calendar day from row 8 onward.
- Issues and pull requests use their closed date when available, otherwise their updated date.
- Commits use the GitHub committer date.
- Multiple activities on the same day are written to separate rows with the date repeated.
- Workdays with activity use `--hours-per-day`, which defaults to `8.0`.
- Weekends and days without activity default to `0` hours.
- Merge commits are excluded from the timesheet.

## Excel Output Format

The generated Excel files preserve the provided template and populate the weekly rows:

- `Date`
- `Type of Day`
- `Performed Project Task(s)`
- `Task Status`
- `Total Hours`
- `Employee Comment`

## Architecture Highlights

- **Async/Await**: Uses Tokio runtime for non-blocking I/O operations
- **Error Handling**: Comprehensive error types using `thiserror` crate
- **Type Safety**: Leverages Rust's type system for compile-time guarantees
- **Modular Design**: Clean separation of concerns across modules
- **Pagination**: Automatic handling of GitHub API pagination (100 items per page)

## Dependencies

- `tokio` - Async runtime
- `reqwest` - HTTP client with rustls-tls for TLS support
- `serde` / `serde_json` - JSON serialization/deserialization
- `chrono` - Date/time handling
- `zip` - Updates template workbook XML inside `.xlsx` files
- `dotenvy` - Environment variable loading
- `thiserror` - Error handling

## Migration from JavaScript

This project was migrated from JavaScript (Node.js) to Rust with the following improvements:

- **Performance**: Compiled binary vs. interpreted runtime
- **Safety**: Compile-time type checking and memory safety
- **Error Handling**: Structured error types instead of runtime errors
- **Concurrency**: Better async/await semantics than JavaScript promises
- **Memory**: Explicit memory management without garbage collection overhead
