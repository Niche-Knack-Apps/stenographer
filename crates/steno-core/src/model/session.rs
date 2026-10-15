use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub display_id: i64,
    pub shell: String,
    pub pid: u32,
    pub ppid: Option<u32>,
    pub tty: Option<String>,
    pub hostname: String,
    pub username: String,
    pub cwd_start: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub exit_code: Option<i32>,
    pub tmux_pane: Option<String>,
    pub ssh_conn: Option<String>,
    pub metadata: Option<String>,
}
