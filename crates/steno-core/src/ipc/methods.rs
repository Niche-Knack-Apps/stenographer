pub struct Method;

impl Method {
    // Daemon
    pub const DAEMON_STATUS: &str = "daemon.status";

    // Query
    pub const QUERY_SEARCH: &str = "query.search";
    pub const QUERY_TAIL: &str = "query.tail";
    pub const QUERY_CONTEXT: &str = "query.context";

    // Session
    pub const SESSION_LIST: &str = "session.list";
    pub const SESSION_SHOW: &str = "session.show";
    pub const SESSION_CURRENT: &str = "session.current";

    // Task (auto-tasks in DB)
    pub const TASK_LIST: &str = "task.list";
    pub const TASK_SHOW: &str = "task.show";
    pub const TASK_DELETE: &str = "task.delete";

    // Privacy
    pub const PRIVACY_MODE_GET: &str = "privacy.mode.get";
    pub const PRIVACY_MODE_SET: &str = "privacy.mode.set";
    pub const PRIVACY_RULES_LIST: &str = "privacy.rules.list";
    pub const PRIVACY_RULES_ADD: &str = "privacy.rules.add";
    pub const PRIVACY_RULES_REMOVE: &str = "privacy.rules.remove";
    pub const PRIVACY_REDACT: &str = "privacy.redact";

    // Import (prime)
    pub const IMPORT_EVENTS: &str = "import.events";

    // Export
    pub const EXPORT_JSON: &str = "export.json";

    // GC
    pub const GC_RUN: &str = "gc.run";
}
