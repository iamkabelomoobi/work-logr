mod client;
mod error;
mod models;
mod prompts;

pub use client::GroqClient;
use models::EnrichmentStats;

use crate::timesheet::model::TimesheetEntry;

pub async fn enrich_entries(client: &GroqClient, entries: &mut [TimesheetEntry]) -> EnrichmentStats {
    let mut stats = EnrichmentStats::default();

    for entry in entries.iter_mut() {
        match client.enrich(entry).await {
            Ok(enrichment) => {
                entry.original_title = Some(entry.title.clone());
                entry.title = enrichment.description;
                entry.ai_category = Some(enrichment.category);
                entry.ai_technical_area = Some(enrichment.technical_area);
                entry.ai_model = Some(client.model().to_string());
                stats.enriched += 1;
            }
            Err(error) => {
                eprintln!(
                    "AI enrichment skipped for {} {}: {}",
                    entry.entry_type, entry.number, error
                );
                stats.failed += 1;
            }
        }
    }

    stats
}
