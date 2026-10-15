use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::Value;

use steno_core::config;
use steno_core::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

pub struct RpcClient {
    stream: UnixStream,
}

impl RpcClient {
    pub fn connect() -> anyhow::Result<Self> {
        let socket_path = config::socket_path();
        let stream = UnixStream::connect(&socket_path).map_err(|_| {
            anyhow::anyhow!(
                "Daemon is not running.\n\
                 \n\
                 First-time setup:  steno install\n\
                 Start the daemon:  steno daemon start\n\
                 Check the logs:    steno daemon logs"
            )
        })?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(Self { stream })
    }

    pub fn call(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let request = JsonRpcRequest::new(method, params);

        let mut request_bytes = serde_json::to_vec(&request)?;
        request_bytes.push(b'\n');
        self.stream.write_all(&request_bytes)?;
        self.stream.flush()?;

        let mut reader = BufReader::new(&self.stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        let response: JsonRpcResponse = serde_json::from_str(&line)?;

        if let Some(error) = response.error {
            anyhow::bail!("{}", error.message);
        }

        Ok(response.result.unwrap_or(Value::Null))
    }
}
