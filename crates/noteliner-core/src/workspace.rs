//! The open project plus its derived link graph — the state the IPC layer
//! and the MCP server share (behind one async mutex, so operations never
//! interleave mid-write).

use std::sync::Arc;

use crate::links::LinkGraph;
use crate::project::Project;

pub struct Workspace {
    pub project: Project,
    pub links: LinkGraph,
}

pub type Shared = Arc<tokio::sync::Mutex<Workspace>>;

impl Workspace {
    pub fn new(project: Project) -> Shared {
        Arc::new(tokio::sync::Mutex::new(Self {
            project,
            links: LinkGraph::default(),
        }))
    }

    pub fn rebuild_links(&mut self) {
        self.links.rebuild(&self.project);
    }

    pub fn scan_links(&mut self, file_id: &str) {
        self.links.scan_file(&self.project, file_id);
    }
}
