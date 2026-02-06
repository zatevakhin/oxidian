use std::time::Instant;
use tracing::{debug, info};
use zerocopy::AsBytes;

use crate::embeddings::EmbeddingModel;
use crate::sqlite::SqliteIndexStore;
use crate::{Error, Result, Vault, VaultIndex, VaultPath};

#[derive(Debug, Clone, PartialEq)]
pub struct NoteSimilarityHit {
    pub source: VaultPath,
    pub target: VaultPath,
    pub score: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteSimilarityReport {
    pub total_notes: usize,
    pub pairs_checked: usize,
    pub hits: Vec<NoteSimilarityHit>,
}

pub(crate) fn note_similarity_report(
    index: &VaultIndex,
    vault: &Vault,
) -> Result<NoteSimilarityReport> {
    let cfg = vault.config();
    let note_paths: Vec<VaultPath> = index.notes_iter_paths().cloned().collect();
    let start = Instant::now();
    info!(
        note_count = note_paths.len(),
        top_k = cfg.similarity_top_k,
        min_score = cfg.similarity_min_score,
        "note similarity report"
    );
    debug!(
        note_count = note_paths.len(),
        top_k = cfg.similarity_top_k,
        min_score = cfg.similarity_min_score,
        max_notes = cfg.similarity_max_notes,
        max_length = cfg.embedding_max_length,
        "note similarity report start"
    );
    if note_paths.len() > cfg.similarity_max_notes {
        return Err(Error::Embedding(format!(
            "note similarity aborted: {} notes exceeds limit {}",
            note_paths.len(),
            cfg.similarity_max_notes
        )));
    }

    let model = EmbeddingModel::load(vault)?;
    let mut store = SqliteIndexStore::open_default(vault)?;
    let embed_start = Instant::now();
    store.ensure_embeddings(vault, index, &model)?;
    debug!(
        elapsed_ms = embed_start.elapsed().as_millis(),
        "note similarity embeddings ready"
    );

    let mut report = NoteSimilarityReport {
        total_notes: note_paths.len(),
        pairs_checked: 0,
        hits: Vec::new(),
    };

    let mut total_candidates = 0usize;
    let mut processed = 0usize;
    for source in &note_paths {
        let embedding = match store.embedding_for_path(source)? {
            Some(v) => v,
            None => continue,
        };
        let candidates = store.knn_for_embedding(embedding.as_bytes(), cfg.similarity_top_k + 1)?;
        total_candidates += candidates.len();
        for (target, distance) in candidates {
            if &target == source {
                continue;
            }
            report.pairs_checked += 1;
            let score = distance_to_cosine(distance);
            if score < cfg.similarity_min_score {
                continue;
            }
            report.hits.push(NoteSimilarityHit {
                source: source.clone(),
                target,
                score,
            });
        }
        processed += 1;
        if processed % 100 == 0 {
            debug!(processed, "note similarity progress");
        }
    }

    report.hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.target.cmp(&b.target))
    });

    let elapsed_ms = start.elapsed().as_millis();
    let avg_candidates = if note_paths.is_empty() {
        0.0
    } else {
        total_candidates as f32 / note_paths.len() as f32
    };
    debug!(
        hits = report.hits.len(),
        pairs_checked = report.pairs_checked,
        total_candidates,
        avg_candidates,
        elapsed_ms,
        "note similarity report complete"
    );
    Ok(report)
}

pub(crate) fn note_similarity_for(
    index: &VaultIndex,
    vault: &Vault,
    source: &VaultPath,
) -> Result<Vec<NoteSimilarityHit>> {
    let cfg = vault.config();
    let start = Instant::now();
    debug!(
        source = source.as_str_lossy(),
        top_k = cfg.similarity_top_k,
        min_score = cfg.similarity_min_score,
        "note similarity query start"
    );
    let model = EmbeddingModel::load(vault)?;
    let mut store = SqliteIndexStore::open_default(vault)?;
    let embed_start = Instant::now();
    store.ensure_embeddings(vault, index, &model)?;
    debug!(
        elapsed_ms = embed_start.elapsed().as_millis(),
        "note similarity embeddings ready"
    );

    let embedding = match store.embedding_for_path(source)? {
        Some(v) => v,
        None => return Ok(Vec::new()),
    };
    let candidates = store.knn_for_embedding(embedding.as_bytes(), cfg.similarity_top_k + 1)?;
    let candidate_count = candidates.len();
    let mut hits = Vec::new();
    for (target, distance) in candidates {
        if &target == source {
            continue;
        }
        let score = distance_to_cosine(distance);
        if score < cfg.similarity_min_score {
            continue;
        }
        hits.push(NoteSimilarityHit {
            source: source.clone(),
            target,
            score,
        });
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    debug!(
        hit_count = hits.len(),
        candidate_count,
        elapsed_ms = start.elapsed().as_millis(),
        "note similarity query complete"
    );
    Ok(hits)
}

fn distance_to_cosine(distance: f32) -> f32 {
    let score = 1.0 - (distance * distance) / 2.0;
    if score < 0.0 {
        0.0
    } else if score > 1.0 {
        1.0
    } else {
        score
    }
}
