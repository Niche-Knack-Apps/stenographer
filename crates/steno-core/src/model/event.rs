use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum SpoolEvent {
    #[serde(rename = "start")]
    Start {
        sid: String,
        pid: u32,
        shell: String,
        tty: Option<String>,
        cwd: String,
        host: String,
        user: String,
        ts: String,
        ppid: Option<u32>,
        tmux_pane: Option<String>,
        ssh_conn: Option<String>,
    },
    #[serde(rename = "pre")]
    Pre {
        sid: String,
        seq: u64,
        cmd: String,
        cwd: String,
        ts: String,
        #[serde(default)]
        ccwd: Option<String>,
    },
    #[serde(rename = "post")]
    Post {
        sid: String,
        seq: u64,
        exit: i32,
        ts: String,
    },
    #[serde(rename = "end")]
    End {
        sid: String,
        exit: Option<i32>,
        ts: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpoolEventType {
    Start,
    Pre,
    Post,
    End,
}

impl SpoolEvent {
    pub fn session_id(&self) -> &str {
        match self {
            Self::Start { sid, .. }
            | Self::Pre { sid, .. }
            | Self::Post { sid, .. }
            | Self::End { sid, .. } => sid,
        }
    }

    pub fn timestamp(&self) -> &str {
        match self {
            Self::Start { ts, .. }
            | Self::Pre { ts, .. }
            | Self::Post { ts, .. }
            | Self::End { ts, .. } => ts,
        }
    }

    pub fn event_type(&self) -> SpoolEventType {
        match self {
            Self::Start { .. } => SpoolEventType::Start,
            Self::Pre { .. } => SpoolEventType::Pre,
            Self::Post { .. } => SpoolEventType::Post,
            Self::End { .. } => SpoolEventType::End,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: i64,
    pub session_id: String,
    pub sequence: u64,
    pub command: String,
    pub command_normalized: Option<String>,
    pub cwd: String,
    pub canonical_cwd: Option<String>,
    pub cd_target_raw: Option<String>,
    pub cd_target_canonical: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<i64>,
    pub git_branch: Option<String>,
    pub git_root: Option<String>,
    pub task_id: Option<i64>,
    pub redacted: bool,
    pub incomplete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrence_count: Option<u64>,
}
