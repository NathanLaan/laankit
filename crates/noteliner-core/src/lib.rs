//! NoteLiner's backend, independent of the UI shell: a project is a folder
//! of notes, and this crate reads it, keeps its index, and serves it to AI
//! assistants over MCP.

pub mod export;
pub mod frontmatter;
pub mod import;
pub mod links;
pub mod mcp;
pub mod project;
pub mod templates;
pub mod tree;
pub mod window_state;
pub mod workspace;
pub mod yaml;
