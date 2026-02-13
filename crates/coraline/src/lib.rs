#![forbid(unsafe_code)]

pub mod config;
pub mod context;
pub mod db;
pub mod extraction;
pub mod graph;
pub mod mcp;
pub mod memory;
pub mod resolution;
pub mod sync;
pub mod tools;
pub mod types;
pub mod utils;
pub mod vectors;

#[derive(Debug, Default)]
pub struct CodeGraph;

impl CodeGraph {
    pub const fn new() -> Self {
        Self
    }
}
