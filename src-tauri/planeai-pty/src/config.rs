use crate::event::SessionId;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum QueuePolicy {
    #[default]
    Block,
    DropOldest,
}

pub struct LocalPtyConfig {
    pub session_id: SessionId,
    /// Shell-wrapped command string (legacy). Used if `program` is None.
    pub command: Option<String>,
    /// Direct executable path (preferred over `command` for argv preservation).
    pub program: Option<String>,
    /// Arguments passed directly to program (no shell interpretation).
    pub args: Vec<String>,
    pub shell: Option<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub read_buffer_size: usize,
    pub coalesce_ms: u64,
    pub coalesce_threshold_bytes: usize,
    pub queue_policy: QueuePolicy,
    pub queue_capacity_bytes: usize,
}

impl Default for LocalPtyConfig {
    fn default() -> Self {
        Self {
            session_id: 0,
            command: None,
            program: None,
            args: Vec::new(),
            shell: None,
            cwd: None,
            env: Vec::new(),
            cols: 80,
            rows: 24,
            read_buffer_size: 16 * 1024,
            coalesce_ms: 4,
            coalesce_threshold_bytes: 4096,
            queue_policy: QueuePolicy::Block,
            queue_capacity_bytes: 512 * 1024,
        }
    }
}
