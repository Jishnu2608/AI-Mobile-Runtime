//! JSON-RPC 2.0 message types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Standard JSON-RPC 2.0 error codes, plus application codes (≥ -32099 … -32000 reserved range).
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    /// Message larger than `MAX_MESSAGE_BYTES`.
    pub const MESSAGE_TOO_LARGE: i64 = -32001;
    /// The method needs `auth.login` first, or the token was wrong.
    pub const UNAUTHENTICATED: i64 = -32002;
    /// Logged in, but the client lacks the required scope.
    pub const FORBIDDEN: i64 = -32003;
    /// Install refused: licences must be accepted first (message lists the ids).
    pub const LICENCE_REQUIRED: i64 = -32010;
    /// Another install job is running.
    pub const BUSY: i64 = -32011;
    /// Not enough free disk space for the plan.
    pub const DISK_SPACE: i64 = -32012;
    /// Release metadata or network failure while preparing.
    pub const UNAVAILABLE: i64 = -32013;
    /// The project folder is not trusted yet (workspace trust, PROJ-001).
    pub const UNTRUSTED: i64 = -32014;
}

/// A request id: number or string (JSON-RPC allows both).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    Num(i64),
    Str(String),
}

/// A JSON-RPC request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Id,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl Request {
    #[must_use]
    pub fn new(id: i64, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Id::Num(id),
            method: method.into(),
            params,
        }
    }
}

/// Error object of a failed call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

/// A JSON-RPC response: exactly one of `result` or `error` is present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    /// `null` when the request id could not be read (parse error / invalid request).
    pub id: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    #[must_use]
    pub fn ok(id: Id, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    #[must_use]
    pub fn err(id: Option<Id>, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Converts into `Ok(result)` or `Err(error)`.
    pub fn into_result(self) -> Result<Value, RpcError> {
        match (self.result, self.error) {
            (_, Some(e)) => Err(e),
            (Some(r), None) => Ok(r),
            (None, None) => Ok(Value::Null),
        }
    }
}

/// Parses one incoming line into a request, or the error response to send back.
pub fn parse_request(line: &[u8]) -> Result<Request, Box<Response>> {
    let value: Value = serde_json::from_slice(line).map_err(|e| {
        Box::new(Response::err(
            None,
            RpcError::new(codes::PARSE_ERROR, format!("parse error: {e}")),
        ))
    })?;
    let id = value
        .get("id")
        .cloned()
        .and_then(|v| serde_json::from_value::<Id>(v).ok());
    let req: Request = serde_json::from_value(value).map_err(|e| {
        Box::new(Response::err(
            id.clone(),
            RpcError::new(codes::INVALID_REQUEST, format!("invalid request: {e}")),
        ))
    })?;
    if req.jsonrpc != "2.0" {
        return Err(Box::new(Response::err(
            Some(req.id),
            RpcError::new(codes::INVALID_REQUEST, "jsonrpc must be \"2.0\""),
        )));
    }
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_round_trip() {
        let r = Request::new(7, "core.ping", Value::Null);
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, r#"{"jsonrpc":"2.0","id":7,"method":"core.ping"}"#);
        assert_eq!(parse_request(s.as_bytes()).unwrap(), r);
    }

    #[test]
    fn string_ids_are_accepted() {
        let r = parse_request(br#"{"jsonrpc":"2.0","id":"a1","method":"x"}"#).unwrap();
        assert_eq!(r.id, Id::Str("a1".into()));
    }

    #[test]
    fn malformed_json_is_parse_error_with_null_id() {
        let e = parse_request(b"{not json").unwrap_err();
        assert_eq!(e.id, None);
        assert_eq!(e.error.unwrap().code, codes::PARSE_ERROR);
    }

    #[test]
    fn missing_method_is_invalid_request_keeping_id() {
        let e = parse_request(br#"{"jsonrpc":"2.0","id":3}"#).unwrap_err();
        assert_eq!(e.id, Some(Id::Num(3)));
        assert_eq!(e.error.unwrap().code, codes::INVALID_REQUEST);
    }

    #[test]
    fn wrong_version_is_invalid_request() {
        let e = parse_request(br#"{"jsonrpc":"1.0","id":1,"method":"x"}"#).unwrap_err();
        assert_eq!(e.error.unwrap().code, codes::INVALID_REQUEST);
    }

    #[test]
    fn response_into_result() {
        assert_eq!(
            Response::ok(Id::Num(1), json!(5)).into_result().unwrap(),
            json!(5)
        );
        let e = Response::err(Some(Id::Num(1)), RpcError::new(-1, "x")).into_result();
        assert_eq!(e.unwrap_err().code, -1);
    }
}
