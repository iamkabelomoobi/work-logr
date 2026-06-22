use super::mapper::build_task_description;
use super::model::TimesheetEntry;
use crate::utils::dates::WeekRange;
use anyhow::{anyhow, Context, Result};
use chrono::{Datelike, NaiveDate, Weekday};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[cfg(test)]
const WORKBOOK_PATH: &str = "xl/workbook.xml";
const WORKSHEET_PATH: &str = "xl/worksheets/sheet1.xml";
const SHARED_STRINGS_PATH: &str = "xl/sharedStrings.xml";

/// Populate a copy of the supplied workbook template without changing layout parts.
pub fn export_to_excel(
    entries: &[TimesheetEntry],
    template_path: &Path,
    output_path: &Path,
    repo: &str,
    user: &str,
    week: &WeekRange,
    hours_per_day: f64,
) -> Result<()> {
    if !template_path.exists() {
        return Err(anyhow!(
            "Template file does not exist: {}",
            template_path.display()
        ));
    }

    let mut input = ZipArchive::new(
        File::open(template_path)
            .with_context(|| format!("Failed to open template: {}", template_path.display()))?,
    )?;

    // Keep every package entry so the output workbook preserves images, styles,
    // merges, print settings, and other template metadata byte-for-byte.
    let mut files = Vec::new();
    let mut shared_strings = String::new();
    let mut worksheet = String::new();

    for index in 0..input.len() {
        let mut file = input.by_index(index)?;
        let name = file.name().to_string();
        let compression = file.compression();
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        if name == SHARED_STRINGS_PATH {
            shared_strings = String::from_utf8(data.clone())
                .context("Template shared strings are not valid UTF-8")?;
        } else if name == WORKSHEET_PATH {
            worksheet =
                String::from_utf8(data.clone()).context("Template worksheet is not valid UTF-8")?;
        }

        files.push((name, compression, data));
    }

    if shared_strings.is_empty() {
        return Err(anyhow!("Template is missing {}", SHARED_STRINGS_PATH));
    }
    if worksheet.is_empty() {
        return Err(anyhow!("Template is missing {}", WORKSHEET_PATH));
    }

    let mut strings = SharedStrings::new(shared_strings)?;
    // Discover write targets from labels in the sheet instead of assuming fixed
    // coordinates. This lets different templates work as long as they expose
    // the same logical labels and table headers.
    let layout = TemplateLayout::discover(&worksheet, strings.values())?;
    let month = week.start.format("%B").to_string();
    let period = format!(
        "{} - {}",
        week.start.format("%Y-%m-%d"),
        week.end.format("%Y-%m-%d")
    );

    let month_idx = strings.push(&month);
    let user_idx = strings.push(user);
    let repo_idx = strings.push(&repo.to_uppercase());
    let period_idx = strings.push(&period);

    let rows = build_timesheet_rows(entries, week, hours_per_day, &mut strings)?;
    let worksheet = replace_worksheet_data(
        &worksheet,
        &layout,
        &[
            ("month", month_idx),
            ("employeename", user_idx),
            ("projectname", repo_idx),
            ("weeklyperiod", period_idx),
        ],
        &rows,
    )?;

    write_workbook(output_path, files, strings.into_xml(), worksheet)?;
    Ok(())
}

fn build_timesheet_rows(
    entries: &[TimesheetEntry],
    week: &WeekRange,
    hours_per_day: f64,
    strings: &mut SharedStrings,
) -> Result<Vec<TimesheetRow>> {
    let mut by_day: BTreeMap<NaiveDate, Vec<&TimesheetEntry>> = BTreeMap::new();

    // Group entries by date only so activities are ordered within the week.
    for entry in entries {
        let entry_date = chrono::DateTime::parse_from_rfc3339(&entry.date)
            .with_context(|| format!("Invalid entry date: {}", entry.date))?
            .naive_utc()
            .date();
        by_day.entry(entry_date).or_default().push(entry);
    }

    let mut rows = Vec::new();
    let mut date = week.start;

    while date <= week.end {
        let day_entries = by_day.get(&date).cloned().unwrap_or_default();
        let is_weekend = matches!(date.weekday(), Weekday::Sat | Weekday::Sun);
        let day_type = if is_weekend { "Weekend" } else { "Work Day" };

        if day_entries.is_empty() {
            rows.push(timesheet_row(date, day_type, "", "", 0.0, strings));
        } else {
            let hours = if is_weekend { 0.0 } else { hours_per_day };
            for entry in day_entries {
                let task = build_task_description(entry);
                let status = status_text(entry);
                rows.push(timesheet_row(date, day_type, &task, status, hours, strings));
            }
        }

        date = date
            .succ_opt()
            .ok_or_else(|| anyhow!("Date overflow while building weekly worksheet"))?;
    }

    Ok(rows)
}

fn timesheet_row(
    date: NaiveDate,
    day_type: &str,
    task: &str,
    status: &str,
    hours: f64,
    strings: &mut SharedStrings,
) -> TimesheetRow {
    TimesheetRow {
        date_serial: excel_date_serial(date),
        day_type_idx: strings.push(day_type),
        task_idx: strings.push(task),
        status_idx: strings.push(status),
        hours,
        comment_idx: strings.push(""),
    }
}

fn status_text(entry: &TimesheetEntry) -> &'static str {
    if matches!(entry.status.as_str(), "closed" | "committed") {
        "Completed"
    } else {
        "In progress"
    }
}

fn excel_date_serial(date: NaiveDate) -> i32 {
    // Excel's 1900 date system uses 1899-12-30 as the serial-day base.
    let base = NaiveDate::from_ymd_opt(1899, 12, 30).expect("valid Excel epoch");
    date.num_days_from_ce() - base.num_days_from_ce()
}

fn replace_worksheet_data(
    worksheet: &str,
    layout: &TemplateLayout,
    shared_string_replacements: &[(&str, usize)],
    rows: &[TimesheetRow],
) -> Result<String> {
    let mut updated = worksheet.to_string();

    // Header values are written next to their discovered labels, not to fixed
    // template coordinates.
    for (field, shared_string_index) in shared_string_replacements {
        let target = layout
            .header_fields
            .iter()
            .find(|target| target.field == *field)
            .ok_or_else(|| anyhow!("Template is missing header label for {}", field))?;
        updated = set_cell(
            &updated,
            &target.cell_ref,
            &target.style,
            Some("s"),
            Some(shared_string_index.to_string()),
        )?;
    }

    if rows.len() > layout.data_rows.len() {
        return Err(anyhow!(
            "Template has {} timesheet row(s), but this week needs {} activity/day row(s)",
            layout.data_rows.len(),
            rows.len()
        ));
    }

    // Clear only the discovered fill columns in existing template rows. We do
    // not create/delete rows or change row/column metadata.
    for row_number in &layout.data_rows {
        updated = clear_timesheet_row_values(&updated, layout, *row_number)?;
    }

    for (row, row_number) in rows.iter().zip(&layout.data_rows) {
        updated = set_cell(
            &updated,
            &layout.cell_ref("date", *row_number)?,
            layout.style("date")?,
            None,
            Some(row.date_serial.to_string()),
        )?;
        updated = set_cell(
            &updated,
            &layout.cell_ref("typeofday", *row_number)?,
            layout.style("typeofday")?,
            Some("s"),
            Some(row.day_type_idx.to_string()),
        )?;
        updated = set_cell(
            &updated,
            &layout.cell_ref("performedprojecttasks", *row_number)?,
            layout.style("performedprojecttasks")?,
            Some("s"),
            Some(row.task_idx.to_string()),
        )?;
        updated = set_cell(
            &updated,
            &layout.cell_ref("taskstatus", *row_number)?,
            layout.style("taskstatus")?,
            Some("s"),
            Some(row.status_idx.to_string()),
        )?;
        updated = set_cell(
            &updated,
            &layout.cell_ref("totalhours", *row_number)?,
            layout.style("totalhours")?,
            None,
            Some(row.hours.to_string()),
        )?;
        updated = set_cell(
            &updated,
            &layout.cell_ref("employeecomment", *row_number)?,
            layout.style("employeecomment")?,
            Some("s"),
            Some(row.comment_idx.to_string()),
        )?;
    }

    Ok(updated)
}

fn clear_timesheet_row_values(
    worksheet: &str,
    layout: &TemplateLayout,
    row_number: u32,
) -> Result<String> {
    let mut updated = worksheet.to_string();

    for column in &layout.columns {
        let cell_ref = format!("{}{}", column.column, row_number);
        if find_cell_range(&updated, &cell_ref).is_some() {
            updated = set_cell(
                &updated,
                &cell_ref,
                &column.style,
                column.cell_type.as_deref(),
                None,
            )?;
        }
    }

    Ok(updated)
}

fn set_cell(
    worksheet: &str,
    cell_ref: &str,
    style: &str,
    cell_type: Option<&str>,
    value: Option<String>,
) -> Result<String> {
    let cell = build_cell_xml(cell_ref, style, cell_type, value.as_deref());
    let mut updated = worksheet.to_string();

    // Replace existing sparse cell XML when Excel already emitted the cell.
    if let Some((cell_start, cell_end)) = find_cell_range(&updated, cell_ref) {
        updated.replace_range(cell_start..cell_end, &cell);
        return Ok(updated);
    }

    // If a blank cell is omitted from sheet XML, insert only that cell inside an
    // existing row. Missing rows are treated as a template error by find_row_range.
    let row_number = row_number_from_cell_ref(cell_ref)?;
    let (row_start, row_end) = find_row_range(&updated, row_number)?;
    let row = &updated[row_start..row_end];
    let insert_at = cell_insert_offset(row, cell_ref)
        .map(|offset| row_start + offset)
        .unwrap_or(row_end - "</row>".len());
    updated.insert_str(insert_at, &cell);
    Ok(updated)
}

fn build_cell_xml(
    cell_ref: &str,
    style: &str,
    cell_type: Option<&str>,
    value: Option<&str>,
) -> String {
    let type_attr = cell_type
        .map(|cell_type| format!(r#" t="{cell_type}""#))
        .unwrap_or_default();

    match value {
        Some(value) => {
            format!(r#"<c r="{cell_ref}" s="{style}"{type_attr}><v>{value}</v></c>"#)
        }
        None => format!(r#"<c r="{cell_ref}" s="{style}"{type_attr}/>"#),
    }
}

fn find_cell_range(worksheet: &str, cell_ref: &str) -> Option<(usize, usize)> {
    let marker = format!(r#"<c r="{cell_ref}""#);
    let cell_start = worksheet.find(&marker)?;
    let relative_cell_end = if let Some(end) = worksheet[cell_start..].find("</c>") {
        end + "</c>".len()
    } else {
        worksheet[cell_start..].find("/>")? + "/>".len()
    };

    Some((cell_start, cell_start + relative_cell_end))
}

fn find_row_range(worksheet: &str, row_number: u32) -> Result<(usize, usize)> {
    let marker = format!(r#"<row r="{row_number}""#);
    let row_start = worksheet
        .find(&marker)
        .ok_or_else(|| anyhow!("Template worksheet is missing row {}", row_number))?;
    let relative_row_end = worksheet[row_start..]
        .find("</row>")
        .ok_or_else(|| anyhow!("Template worksheet row {} is malformed", row_number))?
        + "</row>".len();

    Ok((row_start, row_start + relative_row_end))
}

fn cell_insert_offset(row_xml: &str, cell_ref: &str) -> Option<usize> {
    let target_column = column_index_from_cell_ref(cell_ref)?;
    let mut search_offset = 0;

    // Preserve Excel's normal left-to-right cell ordering when inserting a
    // sparse cell that was omitted from the original XML.
    while let Some(relative_start) = row_xml[search_offset..].find("<c ") {
        let cell_start = search_offset + relative_start;
        let ref_marker = r#"r=""#;
        let ref_start = row_xml[cell_start..].find(ref_marker)? + cell_start + ref_marker.len();
        let ref_end = row_xml[ref_start..].find('"')? + ref_start;
        let existing_ref = &row_xml[ref_start..ref_end];

        if column_index_from_cell_ref(existing_ref)? > target_column {
            return Some(cell_start);
        }

        search_offset = ref_end;
    }

    None
}

fn row_number_from_cell_ref(cell_ref: &str) -> Result<u32> {
    let row = cell_ref
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();

    row.parse()
        .with_context(|| format!("Invalid cell reference: {}", cell_ref))
}

fn column_index_from_cell_ref(cell_ref: &str) -> Option<u32> {
    let mut index = 0;
    for ch in cell_ref.chars().take_while(|ch| ch.is_ascii_alphabetic()) {
        index = index * 26 + (ch.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }

    if index == 0 {
        None
    } else {
        Some(index)
    }
}

fn write_workbook(
    output_path: &Path,
    files: Vec<(String, CompressionMethod, Vec<u8>)>,
    shared_strings: String,
    worksheet: String,
) -> Result<()> {
    let output_file = File::create(output_path)
        .with_context(|| format!("Failed to create output: {}", output_path.display()))?;
    let mut writer = ZipWriter::new(output_file);

    for (name, compression, data) in files {
        let options = SimpleFileOptions::default().compression_method(compression);
        writer.start_file(&name, options)?;

        if name == SHARED_STRINGS_PATH {
            writer.write_all(shared_strings.as_bytes())?;
        } else if name == WORKSHEET_PATH {
            writer.write_all(worksheet.as_bytes())?;
        } else {
            writer.write_all(&data)?;
        }
    }

    writer.finish()?;
    Ok(())
}

struct TimesheetRow {
    date_serial: i32,
    day_type_idx: usize,
    task_idx: usize,
    status_idx: usize,
    hours: f64,
    comment_idx: usize,
}

#[derive(Clone)]
struct FillColumn {
    field: &'static str,
    column: String,
    style: String,
    cell_type: Option<String>,
}

struct HeaderFieldTarget {
    field: &'static str,
    cell_ref: String,
    style: String,
}

struct TemplateLayout {
    header_fields: Vec<HeaderFieldTarget>,
    columns: Vec<FillColumn>,
    data_rows: Vec<u32>,
}

impl TemplateLayout {
    fn discover(worksheet: &str, shared_strings: &[String]) -> Result<Self> {
        let cells = parse_cells(worksheet, shared_strings);
        let columns = discover_fill_columns(&cells)?;
        // The row containing the Date header is treated as the timesheet table
        // header; all existing rows below it are available fill rows.
        let header_row = cells
            .iter()
            .find(|cell| normalize_label(&cell.value) == "date")
            .map(|cell| cell.row)
            .ok_or_else(|| anyhow!("Template is missing a timesheet header row"))?;
        let data_rows = discover_data_rows(worksheet, header_row)?;
        let header_fields = discover_header_fields(&cells)?;

        Ok(Self {
            header_fields,
            columns,
            data_rows,
        })
    }

    fn cell_ref(&self, field: &str, row_number: u32) -> Result<String> {
        let column = self
            .columns
            .iter()
            .find(|column| column.field == field)
            .ok_or_else(|| anyhow!("Template is missing timesheet column {}", field))?;

        Ok(format!("{}{}", column.column, row_number))
    }

    fn style(&self, field: &str) -> Result<&str> {
        self.columns
            .iter()
            .find(|column| column.field == field)
            .map(|column| column.style.as_str())
            .ok_or_else(|| anyhow!("Template is missing style for timesheet column {}", field))
    }
}

#[derive(Clone)]
struct CellInfo {
    cell_ref: String,
    row: u32,
    column: String,
    style: String,
    cell_type: Option<String>,
    value: String,
}

fn discover_fill_columns(cells: &[CellInfo]) -> Result<Vec<FillColumn>> {
    // Header text is normalized so small copy changes like punctuation or
    // parentheses do not break template discovery.
    let header_fields = [
        ("date", ["date"].as_slice()),
        (
            "typeofday",
            ["typeofdayworkdayweekendetc", "typeofday"].as_slice(),
        ),
        (
            "performedprojecttasks",
            ["performedprojecttasks"].as_slice(),
        ),
        ("taskstatus", ["taskstatus"].as_slice()),
        ("totalhours", ["totalhours"].as_slice()),
        ("employeecomment", ["employeecomment"].as_slice()),
    ];

    let mut columns = Vec::new();
    for (field, aliases) in header_fields {
        let header = cells
            .iter()
            .find(|cell| aliases.contains(&normalize_label(&cell.value).as_str()))
            .ok_or_else(|| anyhow!("Template is missing timesheet column header: {}", field))?;
        columns.push(FillColumn {
            field,
            column: header.column.clone(),
            style: header.style.clone(),
            cell_type: header.cell_type.clone(),
        });
    }

    // Use the first data cell below each header as the style/type source. This
    // keeps generated values aligned with the template's existing formatting.
    for column in &mut columns {
        let header_row = cells
            .iter()
            .find(|cell| {
                cell.column == column.column && normalize_label(&cell.value) == column.field
            })
            .map(|cell| cell.row)
            .unwrap_or(0);
        if let Some(data_cell) = cells
            .iter()
            .filter(|cell| cell.row > header_row)
            .find(|cell| cell.column == column.column)
        {
            column.style = data_cell.style.clone();
            column.cell_type = data_cell.cell_type.clone();
        }
    }

    Ok(columns)
}

fn discover_header_fields(cells: &[CellInfo]) -> Result<Vec<HeaderFieldTarget>> {
    let labels = [
        ("month", ["month"].as_slice()),
        ("employeename", ["employeename"].as_slice()),
        ("projectname", ["projectname"].as_slice()),
        ("weeklyperiod", ["weeklyperiod"].as_slice()),
    ];
    let mut targets = Vec::new();

    for (field, aliases) in labels {
        let label = cells
            .iter()
            .find(|cell| aliases.contains(&normalize_label(&cell.value).as_str()))
            .ok_or_else(|| anyhow!("Template is missing header label: {}", field))?;
        // The value cell is expected to be on the same row as the label, usually
        // immediately to the right. If Excel omitted that blank cell, synthesize
        // the adjacent reference and keep the label style as a fallback.
        let target = cells
            .iter()
            .filter(|cell| cell.row == label.row)
            .filter(|cell| cell_col_index(&cell.cell_ref) > cell_col_index(&label.cell_ref))
            .min_by_key(|cell| cell_col_index(&cell.cell_ref))
            .cloned()
            .unwrap_or_else(|| CellInfo {
                cell_ref: format!(
                    "{}{}",
                    column_name(cell_col_index(&label.cell_ref) + 1),
                    label.row
                ),
                row: label.row,
                column: column_name(cell_col_index(&label.cell_ref) + 1),
                style: label.style.clone(),
                cell_type: Some("s".to_string()),
                value: String::new(),
            });

        targets.push(HeaderFieldTarget {
            field,
            cell_ref: target.cell_ref,
            style: target.style,
        });
    }

    Ok(targets)
}

fn discover_data_rows(worksheet: &str, header_row: u32) -> Result<Vec<u32>> {
    let mut rows = parse_row_numbers(worksheet)
        .into_iter()
        .filter(|row| *row > header_row)
        .collect::<Vec<_>>();
    rows.sort_unstable();

    if rows.is_empty() {
        return Err(anyhow!(
            "Template does not have any existing rows below the timesheet header"
        ));
    }

    Ok(rows)
}

fn parse_cells(worksheet: &str, shared_strings: &[String]) -> Vec<CellInfo> {
    let mut cells = Vec::new();
    let mut offset = 0;

    while let Some(relative_start) = worksheet[offset..].find("<c ") {
        let cell_start = offset + relative_start;
        let Some((_, cell_end)) = find_cell_end(worksheet, cell_start) else {
            break;
        };
        let cell_xml = &worksheet[cell_start..cell_end];
        let Some(cell_ref) = attr_value(cell_xml, "r") else {
            offset = cell_end;
            continue;
        };
        let style = attr_value(cell_xml, "s").unwrap_or_else(|| "0".to_string());
        let cell_type = attr_value(cell_xml, "t");
        let raw_value = value_text(cell_xml).unwrap_or_default();
        // Shared-string cells store an index into sharedStrings.xml; resolve
        // that here so discovery can compare human-readable labels.
        let value = if cell_type.as_deref() == Some("s") {
            raw_value
                .parse::<usize>()
                .ok()
                .and_then(|idx| shared_strings.get(idx).cloned())
                .unwrap_or_default()
        } else {
            raw_value
        };

        cells.push(CellInfo {
            row: row_number_from_cell_ref(&cell_ref).unwrap_or(0),
            column: column_name(cell_col_index(&cell_ref)),
            cell_ref,
            style,
            cell_type,
            value,
        });

        offset = cell_end;
    }

    cells
}

fn parse_row_numbers(worksheet: &str) -> Vec<u32> {
    let mut rows = Vec::new();
    let mut offset = 0;

    while let Some(relative_start) = worksheet[offset..].find("<row ") {
        let row_start = offset + relative_start;
        let row_tag_end = worksheet[row_start..]
            .find('>')
            .map(|idx| row_start + idx)
            .unwrap_or(row_start);
        if let Some(row_number) = attr_value(&worksheet[row_start..row_tag_end], "r")
            .and_then(|value| value.parse::<u32>().ok())
        {
            rows.push(row_number);
        }
        offset = row_tag_end.saturating_add(1);
    }

    rows
}

fn parse_shared_string_values(xml: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0;

    while let Some(relative_start) = xml[offset..].find("<si") {
        let si_start = offset + relative_start;
        let Some(relative_end) = xml[si_start..].find("</si>") else {
            break;
        };
        let si_end = si_start + relative_end + "</si>".len();
        let si_xml = &xml[si_start..si_end];
        let mut value = String::new();
        let mut text_offset = 0;

        // Rich text shared strings can contain multiple text runs; concatenate
        // them to recover the visible cell text.
        while let Some(relative_text_start) = si_xml[text_offset..].find("<t") {
            let text_tag_start = text_offset + relative_text_start;
            let Some(text_content_start) = si_xml[text_tag_start..].find('>') else {
                break;
            };
            let content_start = text_tag_start + text_content_start + 1;
            let Some(relative_text_end) = si_xml[content_start..].find("</t>") else {
                break;
            };
            let content_end = content_start + relative_text_end;
            value.push_str(&unescape_xml(&si_xml[content_start..content_end]));
            text_offset = content_end + "</t>".len();
        }

        values.push(value);
        offset = si_end;
    }

    values
}

fn attr_value(xml: &str, attr: &str) -> Option<String> {
    let marker = format!(r#"{attr}=""#);
    let start = xml.find(&marker)? + marker.len();
    let end = xml[start..].find('"')? + start;
    Some(xml[start..end].to_string())
}

fn value_text(cell_xml: &str) -> Option<String> {
    let start = cell_xml.find("<v>")? + "<v>".len();
    let end = cell_xml[start..].find("</v>")? + start;
    Some(cell_xml[start..end].to_string())
}

fn find_cell_end(worksheet: &str, cell_start: usize) -> Option<(usize, usize)> {
    let closed_end = worksheet[cell_start..]
        .find("</c>")
        .map(|end| cell_start + end + "</c>".len());
    let empty_end = worksheet[cell_start..]
        .find("/>")
        .map(|end| cell_start + end + "/>".len());

    match (closed_end, empty_end) {
        (Some(closed), Some(empty)) => Some((cell_start, closed.min(empty))),
        (Some(closed), None) => Some((cell_start, closed)),
        (None, Some(empty)) => Some((cell_start, empty)),
        (None, None) => None,
    }
}

fn normalize_label(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn cell_col_index(cell_ref: &str) -> u32 {
    column_index_from_cell_ref(cell_ref).unwrap_or(0)
}

fn column_name(mut index: u32) -> String {
    if index == 0 {
        return String::new();
    }

    let mut chars = Vec::new();
    while index > 0 {
        index -= 1;
        chars.push((b'A' + (index % 26) as u8) as char);
        index /= 26;
    }
    chars.iter().rev().collect()
}

struct SharedStrings {
    xml: String,
    count: usize,
    unique_count: usize,
    values: Vec<String>,
}

impl SharedStrings {
    fn new(xml: String) -> Result<Self> {
        let count = read_usize_attr(&xml, "count")?;
        let unique_count = read_usize_attr(&xml, "uniqueCount")?;
        let values = parse_shared_string_values(&xml);

        Ok(Self {
            xml,
            count,
            unique_count,
            values,
        })
    }

    fn values(&self) -> &[String] {
        &self.values
    }

    fn push(&mut self, value: &str) -> usize {
        // New output values are appended to sharedStrings.xml, avoiding rewrites
        // of existing template strings and their formatting runs.
        let index = self.unique_count;
        let item = format!("<si><t>{}</t></si>", escape_xml(value));
        let insert_at = self
            .xml
            .rfind("</sst>")
            .expect("shared string XML was validated earlier");
        self.xml.insert_str(insert_at, &item);
        self.count += 1;
        self.unique_count += 1;
        self.values.push(value.to_string());
        index
    }

    fn into_xml(mut self) -> String {
        self.xml = replace_attr(&self.xml, "count", self.count);
        self.xml = replace_attr(&self.xml, "uniqueCount", self.unique_count);
        self.xml
    }
}

fn read_usize_attr(xml: &str, attr: &str) -> Result<usize> {
    let prefix = format!(r#"{attr}=""#);
    let start = xml
        .find(&prefix)
        .ok_or_else(|| anyhow!("sharedStrings.xml is missing {} attribute", attr))?
        + prefix.len();
    let end = xml[start..]
        .find('"')
        .map(|idx| start + idx)
        .ok_or_else(|| anyhow!("sharedStrings.xml has a malformed {} attribute", attr))?;

    xml[start..end]
        .parse()
        .with_context(|| format!("sharedStrings.xml has an invalid {} attribute", attr))
}

fn replace_attr(xml: &str, attr: &str, value: usize) -> String {
    let prefix = format!(r#"{attr}=""#);
    let Some(start) = xml.find(&prefix).map(|idx| idx + prefix.len()) else {
        return xml.to_string();
    };
    let Some(end) = xml[start..].find('"').map(|idx| start + idx) else {
        return xml.to_string();
    };

    let mut output = xml.to_string();
    output.replace_range(start..end, &value.to_string());
    output
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn unescape_xml(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_same_day_commits_and_pull_requests_to_separate_rows() {
        let week = WeekRange {
            start: NaiveDate::from_ymd_opt(2026, 5, 18).expect("valid test date"),
            end: NaiveDate::from_ymd_opt(2026, 5, 24).expect("valid test date"),
        };
        let entries = [
            timesheet_entry("Commit", "", "Add profile support"),
            timesheet_entry("Commit", "", "Make hours configurable"),
            timesheet_entry("Pull Request", "980", "Ship config profiles"),
            timesheet_entry("Pull Request", "976", "Improve Excel formatting"),
        ];
        let mut strings = SharedStrings::new(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="0" uniqueCount="0"></sst>"#
                .to_string(),
        )
        .expect("shared strings should parse");

        let rows = build_timesheet_rows(&entries, &week, 8.0, &mut strings)
            .expect("timesheet rows should build");

        assert_eq!(rows.len(), 10);
        assert!(rows[..4]
            .iter()
            .all(|row| row.date_serial == rows[0].date_serial));
        assert_eq!(
            strings.values()[rows[0].task_idx],
            "Commit: Add profile support"
        );
        assert_eq!(
            strings.values()[rows[1].task_idx],
            "Commit: Make hours configurable"
        );
        assert_eq!(
            strings.values()[rows[2].task_idx],
            "Pull Request #980: Ship config profiles"
        );
        assert_eq!(
            strings.values()[rows[3].task_idx],
            "Pull Request #976: Improve Excel formatting"
        );
        assert!(rows[..4].iter().all(|row| row.hours == 8.0));
        assert_eq!(strings.values()[rows[4].task_idx], "");
        assert_eq!(rows[4].hours, 0.0);
    }

    #[test]
    fn writes_each_weekend_activity_to_a_zero_hour_row() {
        let week = WeekRange {
            start: NaiveDate::from_ymd_opt(2026, 5, 18).expect("valid test date"),
            end: NaiveDate::from_ymd_opt(2026, 5, 24).expect("valid test date"),
        };
        let entries = [
            timesheet_entry_on("Commit", "", "Fix Saturday deploy", "2026-05-24T09:00:00Z"),
            timesheet_entry_on(
                "Pull Request",
                "42",
                "Review weekend release",
                "2026-05-24T11:00:00Z",
            ),
        ];
        let mut strings = SharedStrings::new(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="0" uniqueCount="0"></sst>"#
                .to_string(),
        )
        .expect("shared strings should parse");

        let rows = build_timesheet_rows(&entries, &week, 8.0, &mut strings)
            .expect("timesheet rows should build");

        assert_eq!(rows.len(), 8);
        assert_eq!(rows[6].date_serial, rows[7].date_serial);
        assert_eq!(strings.values()[rows[6].day_type_idx], "Weekend");
        assert_eq!(strings.values()[rows[7].day_type_idx], "Weekend");
        assert_eq!(rows[6].hours, 0.0);
        assert_eq!(rows[7].hours, 0.0);
        assert_eq!(
            strings.values()[rows[7].task_idx],
            "Pull Request #42: Review weekend release"
        );
    }

    #[test]
    fn uses_custom_hours_only_for_workdays_with_activity() {
        let week = WeekRange {
            start: NaiveDate::from_ymd_opt(2026, 5, 18).expect("valid test date"),
            end: NaiveDate::from_ymd_opt(2026, 5, 24).expect("valid test date"),
        };
        let entries = vec![timesheet_entry("Issue", "42", "Populate Excel template")];
        let mut strings = SharedStrings::new(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="0" uniqueCount="0"></sst>"#
                .to_string(),
        )
        .expect("shared strings should parse");

        let rows = build_timesheet_rows(&entries, &week, 7.5, &mut strings)
            .expect("timesheet rows should build");

        assert_eq!(rows[0].hours, 7.5);
        assert_eq!(rows[1].hours, 0.0);
        assert_eq!(rows[5].hours, 0.0);
    }

    #[test]
    fn exports_template_based_workbook() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let output_path = std::env::temp_dir().join(format!("work-logr-{unique}.xlsx"));
        let week = WeekRange {
            start: NaiveDate::from_ymd_opt(2026, 5, 18).expect("valid test date"),
            end: NaiveDate::from_ymd_opt(2026, 5, 24).expect("valid test date"),
        };
        let entries = vec![
            timesheet_entry("Commit", "", "Populate Excel template"),
            timesheet_entry("Pull Request", "42", "Verify separate activity rows"),
        ];

        export_to_excel(
            &entries,
            Path::new("templates/TimesheetTemplate.xlsx"),
            &output_path,
            "nsfas",
            "iamkabelomoobi",
            &week,
            8.0,
        )
        .expect("template export should succeed");

        let mut archive =
            ZipArchive::new(File::open(&output_path).expect("output workbook should exist"))
                .expect("output workbook should be a zip archive");
        let mut shared_strings = String::new();
        archive
            .by_name(SHARED_STRINGS_PATH)
            .expect("output workbook should contain shared strings")
            .read_to_string(&mut shared_strings)
            .expect("shared strings should be readable");
        assert!(shared_strings.contains("Populate Excel template"));
        assert!(shared_strings.contains("Pull Request #42: Verify separate activity rows"));
        assert!(shared_strings.contains("iamkabelomoobi"));

        let output_worksheet = String::from_utf8(read_zip_entry(&output_path, WORKSHEET_PATH))
            .expect("output worksheet should be UTF-8");
        let output_strings = parse_shared_string_values(&shared_strings);
        assert_eq!(
            cell_value(&output_worksheet, "A8"),
            cell_value(&output_worksheet, "A9")
        );
        assert_eq!(
            shared_cell_text(&output_worksheet, &output_strings, "C8"),
            "Commit: Populate Excel template"
        );
        assert_eq!(
            shared_cell_text(&output_worksheet, &output_strings, "C9"),
            "Pull Request #42: Verify separate activity rows"
        );

        let template_workbook =
            read_zip_entry(Path::new("templates/TimesheetTemplate.xlsx"), WORKBOOK_PATH);
        let output_workbook = read_zip_entry(&output_path, WORKBOOK_PATH);
        assert_eq!(template_workbook, output_workbook);

        let template_styles = read_zip_entry(
            Path::new("templates/TimesheetTemplate.xlsx"),
            "xl/styles.xml",
        );
        let output_styles = read_zip_entry(&output_path, "xl/styles.xml");
        assert_eq!(template_styles, output_styles);

        let _ = std::fs::remove_file(output_path);
    }

    fn timesheet_entry(entry_type: &str, number: &str, title: &str) -> TimesheetEntry {
        timesheet_entry_on(entry_type, number, title, "2026-05-18T11:00:00Z")
    }

    fn timesheet_entry_on(
        entry_type: &str,
        number: &str,
        title: &str,
        date: &str,
    ) -> TimesheetEntry {
        TimesheetEntry {
            entry_type: entry_type.to_string(),
            number: number.to_string(),
            title: title.to_string(),
            status: "closed".to_string(),
            closed_at: String::new(),
            created_at: "2026-05-18T09:00:00Z".to_string(),
            updated_at: "2026-05-18T11:00:00Z".to_string(),
            assignees: "iamkabelomoobi".to_string(),
            author: "iamkabelomoobi".to_string(),
            url: format!("https://github.com/example/repo/{entry_type}/{number}"),
            date: date.to_string(),
        }
    }

    fn read_zip_entry(path: &Path, entry: &str) -> Vec<u8> {
        let mut archive =
            ZipArchive::new(File::open(path).expect("workbook should exist")).expect("valid xlsx");
        let mut data = Vec::new();
        archive
            .by_name(entry)
            .expect("entry should exist")
            .read_to_end(&mut data)
            .expect("entry should be readable");
        data
    }

    fn cell_value(worksheet: &str, cell_ref: &str) -> String {
        let (start, end) = find_cell_range(worksheet, cell_ref).expect("cell should exist");
        value_text(&worksheet[start..end]).expect("cell should have a value")
    }

    fn shared_cell_text(worksheet: &str, strings: &[String], cell_ref: &str) -> String {
        let index = cell_value(worksheet, cell_ref)
            .parse::<usize>()
            .expect("shared string index should be numeric");
        strings[index].clone()
    }
}
