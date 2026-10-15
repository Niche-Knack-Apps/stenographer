use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

use steno_core::config::StenoConfig;
use steno_core::db::Database;
use steno_core::ipc::protocol::{JsonRpcRequest, JsonRpcResponse, PARSE_ERROR};

use crate::handler;

pub async fn run(socket_path: PathBuf, db: Arc<Mutex<Database>>, config: StenoConfig) {
    // Remove stale socket
    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).expect("Failed to bind Unix socket");

    // Set socket permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600));
    }

    tracing::info!("RPC server listening on {}", socket_path.display());

    let config = Arc::new(config);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let db = Arc::clone(&db);
                let config = Arc::clone(&config);
                tokio::spawn(async move {
                    handle_connection(stream, db, config).await;
                });
            }
            Err(e) => {
                tracing::warn!("Failed to accept connection: {}", e);
            }
        }
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    db: Arc<Mutex<Database>>,
    config: Arc<StenoConfig>,
) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                    Ok(request) => {
                        let db = db.lock().await;
                        handler::dispatch(&request, &db, &config)
                    }
                    Err(e) => JsonRpcResponse::error(
                        serde_json::Value::Null,
                        PARSE_ERROR,
                        format!("Parse error: {}", e),
                    ),
                };

                let mut response_bytes = serde_json::to_vec(&response).unwrap_or_default();
                response_bytes.push(b'\n');

                if writer.write_all(&response_bytes).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::warn!("Read error: {}", e);
                break;
            }
        }
    }
}
