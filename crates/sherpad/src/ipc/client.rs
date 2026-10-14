//! Minimal IPC client used by `sherpad --call`.

use std::io;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Send one request and return the `result` (or the error message).
pub async fn call(endpoint: &str, method: &str, params: Value) -> io::Result<Result<Value, String>> {
    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(endpoint).await?;
    #[cfg(windows)]
    let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)?;
    let (r, mut w) = tokio::io::split(stream);
    let req = json!({"id": 1, "method": method, "params": params});
    w.write_all(format!("{req}\n").as_bytes()).await?;
    let mut lines = BufReader::new(r).lines();
    while let Some(line) = lines.next_line().await? {
        let v: Value = serde_json::from_str(&line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if v.get("id") == Some(&json!(1)) {
            if let Some(e) = v.get("error") {
                return Ok(Err(e["message"].as_str().unwrap_or("error").to_string()));
            }
            return Ok(Ok(v.get("result").cloned().unwrap_or(Value::Null)));
        }
    }
    Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed without a reply"))
}
