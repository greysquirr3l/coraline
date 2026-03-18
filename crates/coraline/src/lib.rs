#![forbid(unsafe_code)]
// Transitive dependency version conflicts we can't control (base64, getrandom, hashbrown).
#![allow(clippy::multiple_crate_versions)]

pub mod config;
pub mod context;
pub mod db;
pub mod extraction;
pub mod graph;
pub mod logging;
pub mod mcp;
pub mod memory;
pub mod resolution;
pub mod sync;
pub mod tools;
pub mod types;
pub mod utils;
#[cfg(any(feature = "embeddings", feature = "embeddings-dynamic"))]
pub mod vectors;

#[derive(Debug, Default)]
pub struct CodeGraph;

impl CodeGraph {
    pub const fn new() -> Self {
        Self
    }
}
