//! Exports: JSON (sonde's own records) and HAR 1.2.

use serde_json::{Value, json};

use crate::Record;

/// The records as a JSON document `{ "sonde": version, "records": [...] }`.
pub fn to_json(records: &[Record]) -> Value {
    json!({
        "format": "sonde-reqlog",
        "version": 2,
        "records": records,
    })
}

/// The records as an HTTP Archive (HAR 1.2), readable by browsers' tools.
/// Bodies are not kept except error bodies, which go in `response.content`.
pub fn to_har(records: &[Record]) -> Value {
    let entries: Vec<Value> = records
        .iter()
        .map(|r| {
            let headers = |hs: &[crate::Header]| -> Vec<Value> {
                hs.iter().map(|h| json!({"name": h.name, "value": h.value})).collect()
            };
            let query: Vec<Value> =
                r.query.iter().map(|(n, v)| json!({"name": n, "value": v})).collect();
            let mime = r.header("content-type").unwrap_or("application/octet-stream");
            let content = match &r.error_body {
                Some(body) => json!({"size": body.len(), "mimeType": mime, "text": body}),
                None => json!({"size": r.response_bytes.unwrap_or(0), "mimeType": mime}),
            };
            json!({
                "startedDateTime": r.time.to_rfc3339(),
                "time": r.latency_ms,
                "request": {
                    "method": r.method,
                    "url": r.url,
                    "httpVersion": "HTTP/1.1",
                    "headers": headers(&r.request_headers),
                    "queryString": query,
                    "cookies": [],
                    "headersSize": -1,
                    "bodySize": r.request_bytes.map(|b| b as i64).unwrap_or(-1),
                },
                "response": {
                    "status": r.status.unwrap_or(0),
                    "statusText": r.transport_error.clone().unwrap_or_default(),
                    "httpVersion": "HTTP/1.1",
                    "headers": headers(&r.response_headers),
                    "cookies": [],
                    "content": content,
                    "redirectURL": "",
                    "headersSize": -1,
                    "bodySize": r.response_bytes.map(|b| b as i64).unwrap_or(-1),
                },
                "cache": {},
                "timings": {"send": 0, "wait": r.latency_ms, "receive": 0},
                "comment": format!("{} retries={} request-id={}", r.operation, r.retries, r.request_id.clone().unwrap_or_default()),
                "_sonde": {"id": r.id, "endpoint": r.endpoint, "kind": r.operation, "retries": r.retries, "requestId": r.request_id, "hostId": r.host_id},
            })
        })
        .collect();
    json!({
        "log": {
            "version": "1.2",
            "creator": {"name": "sonde", "version": env!("CARGO_PKG_VERSION")},
            "entries": entries,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Header, RequestLog};

    #[test]
    fn har_shape_and_redaction() {
        let log = RequestLog::new(10);
        log.push(Record {
            operation: "GetObject".into(),
            method: "GET".into(),
            url: "https://h/b/k?X-Amz-Signature=secret".into(),
            status: Some(403),
            error_body: Some("<Error><Code>AccessDenied</Code></Error>".into()),
            request_headers: vec![Header::new("x-amz-security-token", "tok")],
            ..Default::default()
        });
        let har = to_har(&log.snapshot());
        let text = har.to_string();
        assert!(!text.contains("secret") && !text.contains("\"tok\""));
        assert_eq!(har["log"]["version"], "1.2");
        assert_eq!(har["log"]["entries"][0]["response"]["status"], 403);
        assert!(
            har["log"]["entries"][0]["response"]["content"]["text"]
                .as_str()
                .unwrap()
                .contains("AccessDenied")
        );
        let j = to_json(&log.snapshot());
        assert_eq!(j["records"].as_array().unwrap().len(), 1);
    }
}
