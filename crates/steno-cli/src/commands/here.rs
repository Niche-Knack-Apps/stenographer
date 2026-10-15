use serde_json::json;

use steno_core::ipc::methods::Method;
use steno_core::model::StoredEvent;

use crate::client::RpcClient;
use crate::commands::picker;
use crate::ui::output;

pub fn run(
    pattern: Option<String>,
    run_flag: bool,
    print_flag: bool,
    limit: i64,
) -> anyhow::Result<()> {
    let action = picker::resolve_action(run_flag, print_flag);

    // Use $PWD from the shell environment — that's what the hook records.
    // current_dir() canonicalizes symlinks and may not match what's stored.
    let cwd = std::env::var("PWD").unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    });

    let mut client = RpcClient::connect()?;

    let events: Vec<StoredEvent> = if let Some(ref p) = pattern {
        let result = client.call(
            Method::QUERY_SEARCH,
            json!({
                "pattern": format!("{}*", p),
                "cwd": cwd,
                "limit": limit,
                "context": 0
            }),
        )?;
        serde_json::from_value(result["events"].clone())?
    } else {
        let result = client.call(
            Method::QUERY_TAIL,
            json!({ "cwd": cwd, "limit": limit, "deduplicate": true }),
        )?;
        serde_json::from_value(result["events"].clone())?
    };

    if events.is_empty() {
        output::warn(&format!("No history for  {}", cwd));
        output::info("Try `steno history` to search across all directories.");
        return Ok(());
    }

    picker::run_picker(events, action)
}
