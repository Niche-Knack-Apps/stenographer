use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionRule {
    pub id: i64,
    pub pattern: String,
    pub replacement: String,
    pub scope: String,
    pub enabled: bool,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreRule {
    pub id: i64,
    pub pattern: String,
    pub match_type: String,
    pub enabled: bool,
}
