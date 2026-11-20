//! Client for the Core's local API (used by the CLI, desktop app and MCP shim).

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::{BufReader, ReadHalf, WriteHalf};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

use crate::frame::{read_frame, write_frame, ReadFrame};
use crate::rpc::{Request, Response, RpcError};
use crate::MAX_MESSAGE_BYTES;

/// `ERROR_PIPE_BUSY`: all server instances are in use; retry shortly.
const ERROR_PIPE_BUSY: i32 = 231;

/// Client-side errors.
#[derive(Debug)]
pub enum ClientError {
    /// No Core is listening on this pipe.
    NotRunning,
    Io(std::io::Error),
    /// The Core answered with a JSON-RPC error.
    Rpc(RpcError),
    /// The answer could not be understood.
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning => write!(f, "the Runtime Core is not running"),
            Self::Io(e) => write!(f, "I/O error talking to the Runtime Core: {e}"),
            Self::Rpc(e) => write!(f, "{e}"),
            Self::Protocol(m) => write!(f, "protocol error: {m}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// A connection to the Core.
#[derive(Debug)]
pub struct Client {
    reader: BufReader<ReadHalf<NamedPipeClient>>,
    writer: WriteHalf<NamedPipeClient>,
    next_id: i64,
}

impl Client {
    /// Connects to the pipe, retrying briefly while the server is busy.
    pub async fn connect(pipe: &str) -> Result<Self, ClientError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let pipe_client = loop {
            match ClientOptions::new().open(pipe) {
                Ok(c) => break c,
                Err(e)
                    if e.raw_os_error() == Some(ERROR_PIPE_BUSY)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ClientError::NotRunning)
                }
                Err(e) => return Err(ClientError::Io(e)),
            }
        };
        let (r, w) = tokio::io::split(pipe_client);
        Ok(Self {
            reader: BufReader::new(r),
            writer: w,
            next_id: 1,
        })
    }

    /// Sends a request and waits for its response.
    pub async fn call_raw(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let req = serde_json::to_vec(&Request::new(id, method, params))
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        write_frame(&mut self.writer, &req)
            .await
            .map_err(ClientError::Io)?;
        match read_frame(&mut self.reader, MAX_MESSAGE_BYTES)
            .await
            .map_err(ClientError::Io)?
        {
            ReadFrame::Message(m) => {
                let resp: Response =
                    serde_json::from_slice(&m).map_err(|e| ClientError::Protocol(e.to_string()))?;
                resp.into_result().map_err(ClientError::Rpc)
            }
            ReadFrame::Closed => Err(ClientError::Protocol(
                "connection closed by the Core".into(),
            )),
            ReadFrame::TooLarge => Err(ClientError::Protocol("response too large".into())),
        }
    }

    /// Authenticates this connection with a token (`auth.login`).
    pub async fn login(&mut self, token: &str) -> Result<crate::methods::LoginResult, ClientError> {
        self.call(
            crate::methods::AUTH_LOGIN,
            serde_json::json!({ "token": token }),
        )
        .await
    }

    /// Connects and logs in.
    pub async fn connect_with_token(pipe: &str, token: &str) -> Result<Self, ClientError> {
        let mut c = Self::connect(pipe).await?;
        c.login(token).await?;
        Ok(c)
    }

    /// Typed call.
    pub async fn call<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<T, ClientError> {
        let v = self.call_raw(method, params).await?;
        serde_json::from_value(v).map_err(|e| ClientError::Protocol(e.to_string()))
    }
}
