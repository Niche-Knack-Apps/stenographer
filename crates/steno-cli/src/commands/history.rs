use serde_json::json;

use steno_core::ipc::methods::Method;
use steno_core::model::StoredEvent;

use crate::client::RpcClient;
use crate::commands::picker;

pub fn run(
    run_flag: bool,
    print_flag: bool,
    limit: i64,
    session: Option<String>,
) -> anyhow::Result<()> {
    let action = picker::resolve_action(run_flag, print_flag);

    let mut client = RpcClient::connect()?;
    let result = client.call(
        Method::QUERY_TAIL,
        json!({ "session": session, "limit": limit, "deduplicate": true }),
    )?;

    let events: Vec<StoredEvent> = serde_json::from_value(result["events"].clone())?;
    picker::run_picker(events, action)
}
