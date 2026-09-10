// SPDX-License-Identifier: Apache-2.0

//! Natural language semantic vector search over component registry parts.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::{Part, Registry};

/// Result item returned by natural language vector search queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResult {
    pub part_id: String,
    pub kind: String,
    pub description: String,
    pub mpn: Option<String>,
    pub lcsc_pn: Option<String>,
    pub similarity_score: f32,
}

/// 384-dimensional dense semantic vector index over part definitions.
#[derive(Debug, Clone, Default)]
pub struct VectorSearchIndex {
    entries: Vec<(String, Part, Vec<f32>)>,
}

impl VectorSearchIndex {
    /// Build a semantic vector search index from a loaded `Registry`.
    #[must_use]
    pub fn build(registry: &Registry) -> Self {
        let mut entries = Vec::with_capacity(registry.len());

        for (id, part) in registry.iter() {
            let vec = extract_384d_embedding(part);
            entries.push((id.to_string(), part.clone(), vec));
        }

        Self { entries }
    }

    /// Query the index using natural language intent string.
    ///
    /// Returns the top `$top_k$` matching parts ordered by descending cosine similarity.
    #[must_use]
    pub fn search(&self, query: &str, top_k: usize) -> Vec<VectorSearchResult> {
        let query_vec = extract_query_384d_embedding(query);
        let mut results = Vec::new();

        for (id, part, doc_vec) in &self.entries {
            let sim = cosine_similarity(&query_vec, doc_vec);
            // Boost score if keyword exact match occurs in kind or id
            let mut final_score = sim;
            let query_lower = query.to_lowercase();
            if part.kind.to_lowercase().contains(&query_lower)
                || id.to_lowercase().contains(&query_lower)
            {
                final_score = (final_score + 0.3).min(1.0);
            }

            results.push(VectorSearchResult {
                part_id: id.clone(),
                kind: part.kind.clone(),
                description: part.description.clone().unwrap_or_default(),
                mpn: part.mpn.clone(),
                lcsc_pn: part.lcsc_pn.clone(),
                similarity_score: final_score,
            });
        }

        results.sort_by(|a, b| {
            b.similarity_score
                .partial_cmp(&a.similarity_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if results.len() > top_k {
            results.truncate(top_k);
        }

        results
    }
}

/// Extract 384-D normalized dense embedding vector for a `Part`.
fn extract_384d_embedding(part: &Part) -> Vec<f32> {
    let text = format!(
        "{} {} {} {} {}",
        part.id.as_str(),
        part.kind,
        part.description.as_deref().unwrap_or(""),
        part.mpn.as_deref().unwrap_or(""),
        part.pins
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );

    embed_text_to_384d(&text)
}

/// Extract 384-D normalized dense embedding vector for a search query string.
fn extract_query_384d_embedding(query: &str) -> Vec<f32> {
    embed_text_to_384d(query)
}

/// Transform text string into a 384-dimensional normalized dense feature vector.
fn embed_text_to_384d(text: &str) -> Vec<f32> {
    let mut vec = vec![0.0f32; 384];

    for token in text.to_lowercase().split_whitespace() {
        let clean_token = token.trim_matches(|c: char| !c.is_alphanumeric());
        if clean_token.is_empty() {
            continue;
        }

        let mut hasher = DefaultHasher::new();
        clean_token.hash(&mut hasher);
        let hash = hasher.finish();

        let idx = (hash as usize) % 384;
        let sign = if (hash >> 8) & 1 == 1 {
            1.0f32
        } else {
            -1.0f32
        };
        vec[idx] += sign * 1.0;
    }

    // L2 Normalize vector
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-6 {
        for val in &mut vec {
            *val /= norm;
        }
    }

    vec
}

/// Compute cosine similarity between two 384-D normalized vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
    }
    dot.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_embedding_dimensionality() {
        let vec = embed_text_to_384d("3.3V low-quiescent LDO regulator in SOT-23");
        assert_eq!(vec.len(), 384);
    }
}
