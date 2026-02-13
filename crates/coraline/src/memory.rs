#![forbid(unsafe_code)]
#![allow(
    clippy::needless_raw_string_hashes,
    clippy::uninlined_format_args,
    clippy::unwrap_used
)]

//! Memory system for project-specific knowledge persistence.
//!
//! Memories are stored as markdown files in `.coraline/memories/` and provide
//! a way to persist project knowledge across sessions.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Memory metadata and content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Manager for project-specific memories.
pub struct MemoryManager {
    memory_dir: PathBuf,
}

impl MemoryManager {
    /// Create a new memory manager for the given project root.
    pub fn new(project_root: &Path) -> io::Result<Self> {
        let memory_dir = project_root.join(".coraline").join("memories");
        fs::create_dir_all(&memory_dir)?;
        Ok(Self { memory_dir })
    }

    /// Get the file path for a memory by name (strips and adds .md extension).
    fn get_memory_path(&self, name: &str) -> PathBuf {
        let name = name.trim_end_matches(".md");
        self.memory_dir.join(format!("{name}.md"))
    }

    /// Write or update a memory.
    pub fn write_memory(&self, name: &str, content: &str) -> io::Result<String> {
        let path = self.get_memory_path(name);
        fs::write(&path, content)?;
        Ok(format!("Memory '{name}' written successfully"))
    }

    /// Read a memory by name.
    pub fn read_memory(&self, name: &str) -> io::Result<String> {
        let path = self.get_memory_path(name);

        if !path.exists() {
            return Ok(format!(
                "Memory '{name}' not found. Consider creating it with write_memory if needed."
            ));
        }

        fs::read_to_string(&path)
    }

    /// List all available memories.
    pub fn list_memories(&self) -> io::Result<Vec<String>> {
        let mut memories = Vec::new();

        if !self.memory_dir.exists() {
            return Ok(memories);
        }

        for entry in fs::read_dir(&self.memory_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file()
                && path.extension().and_then(|s| s.to_str()) == Some("md")
                && let Some(name) = path.file_stem().and_then(|s| s.to_str())
            {
                memories.push(name.to_string());
            }
        }

        memories.sort();
        Ok(memories)
    }

    /// Delete a memory by name.
    pub fn delete_memory(&self, name: &str) -> io::Result<String> {
        let path = self.get_memory_path(name);

        if !path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Memory '{name}' not found"),
            ));
        }

        fs::remove_file(&path)?;
        Ok(format!("Memory '{name}' deleted successfully"))
    }

    /// Check if a memory exists.
    pub fn memory_exists(&self, name: &str) -> bool {
        self.get_memory_path(name).exists()
    }

    /// Get the full path to the memories directory.
    pub fn memory_dir(&self) -> &Path {
        &self.memory_dir
    }
}

/// Create initial memory templates for a new project.
pub fn create_initial_memories(project_root: &Path, project_name: &str) -> io::Result<()> {
    let manager = MemoryManager::new(project_root)?;

    // Project Overview
    let project_overview = format!(
        r"# {project_name} - Project Overview

## Purpose
[Describe the main purpose and goals of this project]

## Architecture
[High-level architecture description]

## Key Components
- [Component 1]: [Description]
- [Component 2]: [Description]

## Technologies
- [Technology stack]

## Entry Points
- [Main files or modules]

## Notes
[Any important notes or context]
"
    );
    manager.write_memory("project_overview", &project_overview)?;

    // Style Conventions
    let style_conventions = r"# Code Style Conventions

## General Principles
- [Principle 1]
- [Principle 2]

## Naming Conventions
- Files: [convention]
- Functions: [convention]
- Variables: [convention]
- Types: [convention]

## Code Organization
- [Organizational pattern]

## Best Practices
- [Practice 1]
- [Practice 2]

## Patterns to Avoid
- [Anti-pattern 1]
- [Anti-pattern 2]
";
    manager.write_memory("style_conventions", style_conventions)?;

    // Suggested Commands
    let suggested_commands = r"# Suggested Development Commands

## Build
```bash
# Development build
cargo build

# Production build
cargo build --release
```

## Test
```bash
# Run all tests
cargo test

# Run specific test
cargo test <test_name>
```

## Run
```bash
# Run the application
cargo run
```

## Other Useful Commands
```bash
# Format code
cargo fmt

# Lint
cargo clippy

# Check types
cargo check
```
";
    manager.write_memory("suggested_commands", suggested_commands)?;

    // Completion Checklist
    let completion_checklist = r"# Feature Completion Checklist

When implementing a new feature, ensure:

- [ ] Code follows style conventions
- [ ] Unit tests written and passing
- [ ] Integration tests added if needed
- [ ] Documentation updated
- [ ] Error handling implemented
- [ ] Edge cases considered
- [ ] Performance implications reviewed
- [ ] Security implications reviewed
- [ ] Code reviewed
- [ ] Memory and resource leaks checked
- [ ] API documentation updated
- [ ] Changelog updated
";
    manager.write_memory("completion_checklist", completion_checklist)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_memory_manager_write_and_read() {
        let temp_dir = TempDir::new().unwrap();
        let manager = MemoryManager::new(temp_dir.path()).unwrap();

        let result = manager
            .write_memory("test_memory", "This is test content")
            .unwrap();
        assert!(result.contains("written successfully"));

        let content = manager.read_memory("test_memory").unwrap();
        assert_eq!(content, "This is test content");
    }

    #[test]
    fn test_memory_manager_handles_md_extension() {
        let temp_dir = TempDir::new().unwrap();
        let manager = MemoryManager::new(temp_dir.path()).unwrap();

        manager.write_memory("test.md", "content").unwrap();
        let content = manager.read_memory("test").unwrap();
        assert_eq!(content, "content");

        let content = manager.read_memory("test.md").unwrap();
        assert_eq!(content, "content");
    }

    #[test]
    fn test_memory_manager_list() {
        let temp_dir = TempDir::new().unwrap();
        let manager = MemoryManager::new(temp_dir.path()).unwrap();

        manager.write_memory("memory1", "content1").unwrap();
        manager.write_memory("memory2", "content2").unwrap();
        manager.write_memory("memory3", "content3").unwrap();

        let memories = manager.list_memories().unwrap();
        assert_eq!(memories.len(), 3);
        assert!(memories.contains(&"memory1".to_string()));
        assert!(memories.contains(&"memory2".to_string()));
        assert!(memories.contains(&"memory3".to_string()));
    }

    #[test]
    fn test_memory_manager_delete() {
        let temp_dir = TempDir::new().unwrap();
        let manager = MemoryManager::new(temp_dir.path()).unwrap();

        manager.write_memory("to_delete", "content").unwrap();
        assert!(manager.memory_exists("to_delete"));

        manager.delete_memory("to_delete").unwrap();
        assert!(!manager.memory_exists("to_delete"));
    }

    #[test]
    fn test_memory_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let manager = MemoryManager::new(temp_dir.path()).unwrap();

        let result = manager.read_memory("nonexistent").unwrap();
        assert!(result.contains("not found"));
    }

    #[test]
    fn test_create_initial_memories() {
        let temp_dir = TempDir::new().unwrap();
        create_initial_memories(temp_dir.path(), "test_project").unwrap();

        let manager = MemoryManager::new(temp_dir.path()).unwrap();
        let memories = manager.list_memories().unwrap();

        assert_eq!(memories.len(), 4);
        assert!(memories.contains(&"project_overview".to_string()));
        assert!(memories.contains(&"style_conventions".to_string()));
        assert!(memories.contains(&"suggested_commands".to_string()));
        assert!(memories.contains(&"completion_checklist".to_string()));

        let overview = manager.read_memory("project_overview").unwrap();
        assert!(overview.contains("test_project"));
    }
}
