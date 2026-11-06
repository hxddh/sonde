use serde::{Deserialize, Serialize};

/// What went wrong, in categories the UI can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The service answered with an error status.
    Http,
    /// No response: DNS, TCP, TLS, timeout.
    Transport,
    /// Credentials could not be resolved.
    Credentials,
    /// The endpoint or the request is misconfigured.
    Config,
    /// The response could not be decoded.
    Decode,
    /// The job was cancelled.
    Cancelled,
    /// Refused locally: read-only mode, missing confirmation.
    Refused,
}

/// An error with everything needed to diagnose it: the service's code and
/// message, its request ids, the raw body, and the id of the request
/// record (so the UI can jump to it in the inspector).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{}", self.display())]
pub struct S3Error {
    pub kind: ErrorKind,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub message: String,
    pub request_id: Option<String>,
    pub host_id: Option<String>,
    /// The request log record of the failed request.
    pub record_id: Option<u64>,
    pub body: Option<String>,
}

impl S3Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        S3Error {
            kind,
            status: None,
            code: None,
            message: message.into(),
            request_id: None,
            host_id: None,
            record_id: None,
            body: None,
        }
    }

    pub fn config(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Config, m)
    }
    pub fn decode(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Decode, m)
    }
    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "cancelled")
    }
    pub fn refused(m: impl Into<String>) -> Self {
        Self::new(ErrorKind::Refused, m)
    }

    fn display(&self) -> String {
        let mut s = String::new();
        if let Some(code) = &self.code {
            s.push_str(code);
            if let Some(st) = self.status {
                s.push_str(&format!(" ({st})"));
            }
            if !self.message.is_empty() && &self.message != code {
                s.push_str(": ");
                s.push_str(&self.message);
            }
        } else if let Some(st) = self.status {
            s.push_str(&format!("HTTP {st}"));
            if !self.message.is_empty() {
                s.push_str(": ");
                s.push_str(&self.message);
            }
        } else {
            s.push_str(&self.message);
        }
        if let Some(id) = &self.request_id {
            s.push_str(&format!(" [request-id {id}]"));
        }
        s
    }

    /// Whether it is the service's way of saying "slow down / try later".
    pub fn is_throttle(&self) -> bool {
        matches!(self.status, Some(503) | Some(429))
            || matches!(
                self.code.as_deref(),
                Some(
                    "SlowDown"
                        | "Throttling"
                        | "ThrottlingException"
                        | "TooManyRequests"
                        | "RequestLimitExceeded"
                )
            )
    }

    pub fn is_not_found(&self) -> bool {
        self.status == Some(404)
    }

    /// The error a non-2xx response stands for.
    pub fn from_response(
        status: u16,
        body: &[u8],
        request_id: Option<String>,
        host_id: Option<String>,
        record_id: u64,
    ) -> Self {
        let text = String::from_utf8_lossy(body).to_string();
        let parsed = crate::xml::parse_error(&text);
        let (code, message, rid, hid) = match parsed {
            Some(e) => (
                Some(e.code),
                e.message.unwrap_or_default(),
                e.request_id,
                e.host_id,
            ),
            None => (None, http_reason(status).to_string(), None, None),
        };
        S3Error {
            kind: ErrorKind::Http,
            status: Some(status),
            code,
            message,
            request_id: request_id.or(rid),
            host_id: host_id.or(hid),
            record_id: Some(record_id),
            body: if text.is_empty() {
                None
            } else {
                Some(reqlog::redact::redact_text(&text))
            },
        }
    }
}

pub fn http_reason(status: u16) -> &'static str {
    match status {
        301 => "Moved Permanently (wrong region or addressing style?)",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        412 => "Precondition Failed",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable() {
        let body = b"<?xml version=\"1.0\"?><Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message><Key>a</Key><RequestId>4442587FB7D0A2F9</RequestId></Error>";
        let e = S3Error::from_response(404, body, None, None, 7);
        assert_eq!(
            e.to_string(),
            "NoSuchKey (404): The specified key does not exist. [request-id 4442587FB7D0A2F9]"
        );
        assert_eq!(e.record_id, Some(7));
        let e = S3Error::from_response(404, b"", Some("R".into()), None, 1);
        assert_eq!(e.to_string(), "HTTP 404: Not Found [request-id R]");
        let e = S3Error::from_response(503, b"<Error><Code>SlowDown</Code><Message>Please reduce your request rate.</Message></Error>", None, None, 1);
        assert!(e.is_throttle());
    }
}
