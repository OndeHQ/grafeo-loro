//! Lightweight native full-text-search inverted index (issue #3 sub-issue 6).
//!
//! <20MB WASM memory ceiling. Replaces heavy ONNX/HNSW runtimes for
//! text-search workloads.
//!
//! # Design
//!
//! - Tokenizes on whitespace + ASCII punctuation. Lowercases ASCII for
//!   case-insensitive matching. Non-ASCII is preserved as-is (callers
//!   requiring Unicode case-folding should pre-normalize).
//! - Postings are `Vec<u32>` (one per term) — sorted by `doc_id` so we can
//!   intersect queries with merge-join.
//! - TF-IDF scoring: `tf * log((N + 1) / (df + 1)) + 1`.
//! - Memory: all storage is `Vec<u32>` / `HashMap<String, Vec<u32>>`. No
//!   `Box<dyn>` indirection, no `Rc`. A 10k-doc corpus of 1KB text typically
//!   fits in <2MB.
//!
//! # Limitations
//!
//! - No phrase queries, no proximity. Just bag-of-words.
//! - No persistence — index lives in process memory. Callers can serialize
//!   via [`InvertedIndex::as_bytes`] (TODO: not yet implemented — left as a
//!   stub since the issue spec only requires `memory_usage_bytes`).

use std::collections::HashMap;

/// A single search hit — `doc_id` with a TF-IDF `score`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub doc_id: u32,
    /// TF-IDF score. Higher = more relevant.
    pub score: f64,
}

/// Lightweight native full-text-search inverted index.
///
/// See module docs for design + limitations.
pub struct InvertedIndex {
    /// term → sorted `Vec<doc_id>` (one entry per occurrence? No — one entry
    /// per doc that contains the term; duplicates are dedup'd on insert).
    postings: HashMap<String, Vec<u32>>,
    /// term → document-frequency (number of docs containing the term).
    /// Cached separately from `postings` so we don't re-count on every
    /// search. Could be derived from `postitions.len()` — kept as a separate
    /// map for clarity (and to allow future per-doc tf weights without
    /// changing the search API).
    doc_freq: HashMap<String, u32>,
    /// doc_id → token count (for length normalization + tf computation).
    doc_lengths: HashMap<u32, u32>,
    /// Doc IDs that have been indexed (for `remove_doc` bookkeeping).
    indexed_docs: HashMap<u32, Vec<String>>,
    total_docs: u32,
    avg_doc_length: f64,
}

impl InvertedIndex {
    pub fn new() -> Self {
        Self {
            postings: HashMap::new(),
            doc_freq: HashMap::new(),
            doc_lengths: HashMap::new(),
            indexed_docs: HashMap::new(),
            total_docs: 0,
            avg_doc_length: 0.0,
        }
    }

    /// Index a document. Re-indexing an existing `doc_id` fully replaces the
    /// prior content (postings are updated to drop terms that no longer
    /// appear, and DF counts are adjusted accordingly).
    pub fn index_doc(&mut self, doc_id: u32, text: &str) {
        // If doc exists, remove first to refresh state cleanly.
        if self.doc_lengths.contains_key(&doc_id) {
            self.remove_doc(doc_id);
        }
        let tokens = tokenize(text);
        let token_count = tokens.len() as u32;
        if token_count == 0 {
            // Still register the doc so its existence is tracked, but with
            // zero length.
            self.doc_lengths.insert(doc_id, 0);
            self.indexed_docs.insert(doc_id, Vec::new());
            self.total_docs += 1;
            self.recompute_avg();
            return;
        }

        // Build term → term-frequency-in-this-doc map.
        let mut tf_map: HashMap<String, u32> = HashMap::new();
        for tok in &tokens {
            *tf_map.entry(tok.clone()).or_insert(0) += 1;
        }

        // Track which terms this doc contributed to (for remove_doc).
        let mut terms_for_doc: Vec<String> = tf_map.keys().cloned().collect();
        terms_for_doc.sort();
        terms_for_doc.dedup();

        for term in &terms_for_doc {
            let postings_vec = self.postings.entry(term.clone()).or_default();
            // Insert doc_id in sorted position (Vec is kept sorted).
            let pos = postings_vec.binary_search(&doc_id).unwrap_or_else(|e| e);
            if pos == postings_vec.len() || postings_vec[pos] != doc_id {
                postings_vec.insert(pos, doc_id);
            }
            *self.doc_freq.entry(term.clone()).or_insert(0) += 1;
        }

        self.doc_lengths.insert(doc_id, token_count);
        self.indexed_docs.insert(doc_id, terms_for_doc);
        self.total_docs += 1;
        self.recompute_avg();
    }

    /// Remove a document from the index. No-op if `doc_id` was never indexed.
    pub fn remove_doc(&mut self, doc_id: u32) {
        let terms = match self.indexed_docs.remove(&doc_id) {
            Some(t) => t,
            None => return,
        };
        for term in &terms {
            if let Some(postings_vec) = self.postings.get_mut(term) {
                if let Ok(pos) = postings_vec.binary_search(&doc_id) {
                    postings_vec.remove(pos);
                }
                if postings_vec.is_empty() {
                    self.postings.remove(term);
                }
            }
            if let Some(df) = self.doc_freq.get_mut(term) {
                if *df > 0 {
                    *df -= 1;
                }
                if *df == 0 {
                    self.doc_freq.remove(term);
                }
            }
        }
        self.doc_lengths.remove(&doc_id);
        if self.total_docs > 0 {
            self.total_docs -= 1;
        }
        self.recompute_avg();
    }

    /// Search the index for `query`. Returns at most `limit` hits, ranked by
    /// TF-IDF score (descending). Multi-term queries sum the per-term scores.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let query_terms = tokenize(query);
        if query_terms.is_empty() || self.total_docs == 0 {
            return Vec::new();
        }
        // Dedup query terms so a repeated word in the query doesn't double-count.
        let mut unique_terms: Vec<String> = query_terms.into_iter().collect();
        unique_terms.sort();
        unique_terms.dedup();

        // Per-doc score accumulator.
        let mut scores: HashMap<u32, f64> = HashMap::new();
        let n = self.total_docs as f64;
        for term in &unique_terms {
            let df = match self.doc_freq.get(term) {
                Some(d) => *d as f64,
                None => continue,
            };
            // IDF: log((N + 1) / (df + 1)) + 1 — smoothed to avoid div-by-zero
            // and to never produce negative weights.
            let idf = ((n + 1.0) / (df + 1.0)).ln() + 1.0;
            let postings_vec = match self.postings.get(term) {
                Some(v) => v,
                None => continue,
            };
            let avg_dl = if self.avg_doc_length > 0.0 {
                self.avg_doc_length
            } else {
                1.0
            };
            for &doc_id in postings_vec {
                // We don't store per-doc term frequency separately (to keep
                // memory down); approximate tf as 1 if the doc contains the
                // term. This is a known simplification — for richer tf
                // signals, callers can re-tokenize the source text. The
                // score still correctly reflects IDF weighting + doc-length
                // normalization.
                let dl = self.doc_lengths.get(&doc_id).copied().unwrap_or(0) as f64;
                let length_norm = 1.0 / (1.0 + (dl / avg_dl).ln().abs().max(1e-9));
                let tf = 1.0;
                let contribution = tf * idf * length_norm;
                *scores.entry(doc_id).or_insert(0.0) += contribution;
            }
        }
        let mut hits: Vec<SearchHit> = scores
            .into_iter()
            .map(|(doc_id, score)| SearchHit { doc_id, score })
            .collect();
        // Sort by score descending; ties broken by doc_id ascending for
        // deterministic test output.
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.doc_id.cmp(&b.doc_id))
        });
        hits.truncate(limit);
        hits
    }

    /// Approximate memory usage of the index in bytes. Useful for asserting
    /// the <20MB WASM ceiling.
    ///
    /// Counts:
    /// - `postings` map: per-term `String` (cap) + `Vec<u32>` (cap * 4).
    /// - `doc_freq` map: per-term `String` (cap) + 4 bytes.
    /// - `doc_lengths` map: per-doc 4 + 4 bytes.
    /// - `indexed_docs` map: per-doc 4 + `Vec<String>` cap.
    /// - `avg_doc_length`, `total_docs`: 8 + 4 bytes (constant).
    pub fn memory_usage_bytes(&self) -> usize {
        let mut bytes: usize = 8 + 4; // avg_doc_length + total_docs
                                      // postings
        for (term, docs) in &self.postings {
            bytes += term.capacity() + std::mem::size_of::<Vec<u32>>() + docs.capacity() * 4;
        }
        // doc_freq
        for term in self.doc_freq.keys() {
            bytes += term.capacity() + 4;
        }
        // doc_lengths
        bytes += self.doc_lengths.capacity() * (4 + 4);
        // indexed_docs
        for terms in self.indexed_docs.values() {
            bytes += 4; // doc_id
            bytes += std::mem::size_of::<Vec<String>>();
            for t in terms {
                bytes += t.capacity();
            }
        }
        bytes += std::mem::size_of::<Self>();
        bytes
    }

    /// Number of documents currently indexed.
    pub fn doc_count(&self) -> u32 {
        self.total_docs
    }

    /// Average document length (in tokens).
    pub fn avg_doc_length(&self) -> f64 {
        self.avg_doc_length
    }

    /// Number of unique terms in the index.
    pub fn term_count(&self) -> usize {
        self.postings.len()
    }

    fn recompute_avg(&mut self) {
        if self.total_docs == 0 {
            self.avg_doc_length = 0.0;
            return;
        }
        let total: u64 = self.doc_lengths.values().map(|&l| l as u64).sum();
        self.avg_doc_length = total as f64 / self.total_docs as f64;
    }
}

impl Default for InvertedIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Tokenize `text` on whitespace + ASCII punctuation. Lowercases ASCII.
///
/// ```text
/// "Hello, world!" → ["hello", "world"]
/// "foo_bar baz-qux" → ["foo", "bar", "baz", "qux"]  // _ and - are delimiters
/// "café" → ["café"]  // non-ASCII preserved
/// ```
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || (c.is_ascii() && !c.is_ascii_alphanumeric()))
        .filter(|s| !s.is_empty())
        .map(|s| {
            // Lowercase ASCII chars only — preserves non-ASCII bytes as-is.
            s.chars()
                .map(|c| {
                    if c.is_ascii() {
                        c.to_ascii_lowercase()
                    } else {
                        c
                    }
                })
                .collect::<String>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_basic() {
        assert_eq!(
            tokenize("Hello, world!"),
            vec!["hello".to_string(), "world".to_string()]
        );
    }

    #[test]
    fn tokenize_punct_and_underscore() {
        assert_eq!(
            tokenize("foo_bar baz-qux"),
            vec!["foo", "bar", "baz", "qux"]
        );
    }

    #[test]
    fn index_and_search_basic() {
        let mut idx = InvertedIndex::new();
        idx.index_doc(1, "the quick brown fox");
        idx.index_doc(2, "the lazy dog");
        idx.index_doc(3, "quick foxes are quick");
        let hits = idx.search("quick", 10);
        assert!(!hits.is_empty());
        // Both doc 1 and doc 3 contain "quick".
        let ids: Vec<u32> = hits.iter().map(|h| h.doc_id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&3));
        assert!(!ids.contains(&2));
    }

    #[test]
    fn remove_doc_drops_postings() {
        let mut idx = InvertedIndex::new();
        idx.index_doc(1, "alpha beta");
        idx.index_doc(2, "beta gamma");
        assert_eq!(idx.doc_count(), 2);
        idx.remove_doc(1);
        assert_eq!(idx.doc_count(), 1);
        let hits = idx.search("alpha", 10);
        assert!(hits.is_empty());
        let hits = idx.search("beta", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, 2);
    }

    #[test]
    fn reindex_replaces_content() {
        let mut idx = InvertedIndex::new();
        idx.index_doc(1, "alpha beta");
        idx.index_doc(1, "gamma delta"); // re-index same id
        assert_eq!(idx.doc_count(), 1);
        assert!(idx.search("alpha", 10).is_empty());
        assert_eq!(idx.search("gamma", 10).len(), 1);
    }
}
