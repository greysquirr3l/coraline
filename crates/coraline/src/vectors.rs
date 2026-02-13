#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::doc_markdown,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::items_after_statements,
    clippy::redundant_closure_for_method_calls,
    clippy::uninlined_format_args,
    clippy::unwrap_used
)]

//! Vector embeddings for semantic code search.
//!
//! This module provides functionality to generate vector embeddings for code symbols
//! and semantic search using cosine similarity.

use std::io;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::types::SearchResult;

/// Model identifier for nomic-embed-text-v1.5
pub const DEFAULT_MODEL: &str = "nomic-embed-text-v1.5";

/// Expected embedding dimension for nomic-embed-text-v1.5
pub const EMBEDDING_DIM: usize = 384;

/// Vector embedding manager (placeholder for future ONNX integration).
pub struct VectorManager {
    model_name: String,
}

impl VectorManager {
    /// Create a new VectorManager with the specified ONNX model.
    ///
    /// # Arguments
    ///
    /// * `_model_path` - Path to the ONNX model file (currently unused)
    ///
    /// # Returns
    ///
    /// A new VectorManager instance.
    ///
    /// # Note
    ///
    /// Full ONNX integration is TODO. This currently returns a placeholder.
    pub fn new(_model_path: &Path) -> io::Result<Self> {
        // TODO: Integrate ort crate properly when API is stable
        // let session = Session::builder()
        //     .with_optimization_level(GraphOptimizationLevel::Level3)?
        //     .commit_from_file(model_path)?;

        Ok(Self {
            model_name: DEFAULT_MODEL.to_string(),
        })
    }

    /// Generate an embedding vector for the given text.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to embed
    ///
    /// # Returns
    ///
    /// A 384-dimensional embedding vector or an error.
    ///
    /// # Note
    ///
    /// This currently returns a zero vector. Full implementation requires:
    /// 1. Tokenization using nomic-embed tokenizer
    /// 2. Converting tokens to input IDs
    /// 3. Running inference through ONNX model
    /// 4. Extracting and normalizing the output embedding
    pub fn embed(&self, _text: &str) -> io::Result<Vec<f32>> {
        // TODO: Implement full ONNX inference pipeline
        Ok(vec![0.0; EMBEDDING_DIM])
    }

    /// Get the model name.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }
}

/// Store an embedding vector for a node in the database.
///
/// # Arguments
///
/// * `conn` - Database connection
/// * `node_id` - ID of the node
/// * `embedding` - The embedding vector
/// * `model_name` - Name of the model used to generate the embedding
pub fn store_embedding(
    conn: &Connection,
    node_id: &str,
    embedding: &[f32],
    model_name: &str,
) -> io::Result<()> {
    // Convert f32 slice to bytes
    let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|&f| f.to_le_bytes()).collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    conn.execute(
        "INSERT OR REPLACE INTO vectors (node_id, embedding, model, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![node_id, embedding_bytes, model_name, now],
    )
    .map_err(|e| io::Error::other(format!("Failed to store embedding: {}", e)))?;

    Ok(())
}

/// Load an embedding vector from the database.
///
/// # Arguments
///
/// * `conn` - Database connection
/// * `node_id` - ID of the node
///
/// # Returns
///
/// The embedding vector or None if not found.
pub fn load_embedding(conn: &Connection, node_id: &str) -> io::Result<Option<Vec<f32>>> {
    let mut stmt = conn
        .prepare("SELECT embedding FROM vectors WHERE node_id = ?1")
        .map_err(|e| io::Error::other(format!("Failed to prepare query: {}", e)))?;

    let mut rows = stmt
        .query(params![node_id])
        .map_err(|e| io::Error::other(format!("Failed to query: {}", e)))?;

    match rows.next().map_err(io::Error::other)? {
        Some(row) => {
            let bytes: Vec<u8> = row.get(0).map_err(io::Error::other)?;

            // Convert bytes back to f32 slice
            let embedding: Vec<f32> = bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();
            Ok(Some(embedding))
        }
        None => Ok(None),
    }
}

/// Calculate cosine similarity between two vectors.
///
/// # Arguments
///
/// * `a` - First vector
/// * `b` - Second vector
///
/// # Returns
///
/// Cosine similarity in range [-1, 1], where 1 means identical direction.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

/// Search for nodes similar to the query embedding.
///
/// # Arguments
///
/// * `conn` - Database connection
/// * `query_embedding` - The query embedding vector
/// * `limit` - Maximum number of results to return
/// * `min_similarity` - Minimum cosine similarity threshold (0.0 to 1.0)
///
/// # Returns
///
/// A vector of SearchResult ordered by similarity (highest first).
pub fn search_similar(
    conn: &Connection,
    query_embedding: &[f32],
    limit: usize,
    min_similarity: f32,
) -> io::Result<Vec<SearchResult>> {
    let mut stmt = conn
        .prepare(
            "SELECT v.node_id, v.embedding,
                         n.id, n.kind, n.name, n.qualified_name, n.file_path, n.language,
                         n.start_line, n.end_line, n.start_column, n.end_column,
                         n.docstring, n.signature, n.visibility,
                         n.is_exported, n.is_async, n.is_static, n.is_abstract,
                         n.decorators, n.type_parameters
                  FROM vectors v
                  JOIN nodes n ON v.node_id = n.id",
        )
        .map_err(|e| io::Error::other(format!("Failed to prepare query: {}", e)))?;

    let rows = stmt
        .query_map([], |row| {
            let embedding_bytes: Vec<u8> = row.get(1)?;

            // Convert bytes to f32 vector
            let embedding: Vec<f32> = embedding_bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();

            let similarity = cosine_similarity(query_embedding, &embedding);

            // Parse node from row (offset by 2 since we have node_id and embedding first)
            use crate::types::{Language, Node, NodeKind};

            let node = Node {
                id: row.get(2)?,
                kind: serde_json::from_str(&format!("\"{}\"", row.get::<_, String>(3)?))
                    .unwrap_or(NodeKind::Function),
                name: row.get(4)?,
                qualified_name: row.get(5)?,
                file_path: row.get(6)?,
                language: serde_json::from_str(&format!("\"{}\"", row.get::<_, String>(7)?))
                    .unwrap_or(Language::TypeScript),
                start_line: row.get(8)?,
                end_line: row.get(9)?,
                start_column: row.get(10)?,
                end_column: row.get(11)?,
                docstring: row.get(12)?,
                signature: row.get(13)?,
                visibility: row
                    .get::<_, Option<String>>(14)?
                    .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok()),
                is_exported: row.get(15)?,
                is_async: row.get(16)?,
                is_static: row.get(17)?,
                is_abstract: row.get(18)?,
                decorators: row
                    .get::<_, Option<String>>(19)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                type_parameters: row
                    .get::<_, Option<String>>(20)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                updated_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
            };

            Ok((similarity, node))
        })
        .map_err(|e| io::Error::other(format!("Failed to execute query: {}", e)))?;

    let mut results: Vec<_> = rows
        .filter_map(|r| r.ok())
        .filter(|(sim, _)| *sim >= min_similarity)
        .collect();

    // Sort by similarity (highest first)
    results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Take top N and convert to SearchResult
    Ok(results
        .into_iter()
        .take(limit)
        .map(|(similarity, node)| SearchResult {
            node,
            score: similarity,
            highlights: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }
}
