mod client;
mod error;
mod models;
mod prompts;

pub use client::GroqClient;
use models::EnrichmentStats;

use crate::timesheet::model::TimesheetEntry;

pub async fn enrich_entries(
    client: &GroqClient,
    entries: &mut [TimesheetEntry],
    batch_size: usize,
) -> EnrichmentStats {
    let mut stats = EnrichmentStats::default();

    for batch in entries.chunks_mut(batch_size) {
        match client.enrich_batch(batch).await {
            Ok(enrichments) => {
                for (entry, enrichment) in batch.iter_mut().zip(enrichments) {
                    entry.original_title = Some(entry.title.clone());
                    entry.title = enrichment.description;
                    entry.ai_category = Some(enrichment.category);
                    entry.ai_technical_area = Some(enrichment.technical_area);
                    entry.ai_model = Some(client.model().to_string());
                    stats.enriched += 1;
                }
            }
            Err(error) => {
                eprintln!(
                    "AI enrichment skipped for batch of {} entr{}: {}",
                    batch.len(),
                    if batch.len() == 1 { "y" } else { "ies" },
                    error
                );
                stats.failed += batch.len();
            }
        }
    }

    stats
}
