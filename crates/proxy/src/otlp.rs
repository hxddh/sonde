//! A session as OpenTelemetry traces (OTLP/JSON): one trace, each operation
//! a span, its requests spans under it, named and attributed as the
//! OpenTelemetry conventions for AWS SDK calls name them — so Grafana Tempo,
//! Jaeger or Honeycomb show it beside the program's own traces.

use std::hash::{Hash, Hasher};

use serde_json::{Value, json};

use crate::episode::{Outcome, episodes};
use crate::session::Session;

fn hex(n: u64, bytes: usize) -> String {
    let s = format!("{n:016x}");
    s[16 - (bytes * 2).min(16)..].to_string()
}

fn hash(parts: &[&str]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

fn attr(key: &str, v: Value) -> Value {
    let value = match v {
        Value::String(s) => json!({ "stringValue": s }),
        Value::Number(n) if n.is_i64() || n.is_u64() => json!({ "intValue": n.to_string() }),
        Value::Number(n) => json!({ "doubleValue": n }),
        Value::Bool(b) => json!({ "boolValue": b }),
        other => json!({ "stringValue": other.to_string() }),
    };
    json!({ "key": key, "value": value })
}

fn nanos(t: chrono::DateTime<chrono::Utc>) -> String {
    t.timestamp_nanos_opt().unwrap_or(0).to_string()
}

/// The session as an OTLP/JSON `ExportTraceServiceRequest`.
pub fn traces(s: &Session) -> Value {
    let created = s.header.created.to_rfc3339();
    let trace_hi = hash(&["sonde", &created, &s.header.endpoint]);
    let trace_lo = hash(&[&created, s.header.command.as_deref().unwrap_or("")]);
    let trace_id = format!("{trace_hi:016x}{trace_lo:016x}");
    let eps = episodes(&s.records);
    let by_id: std::collections::HashMap<u64, &reqlog::Record> =
        s.records.iter().map(|r| (r.id, r)).collect();
    let mut spans = Vec::new();
    for (i, e) in eps.iter().enumerate() {
        let span_id = hex(hash(&["op", &i.to_string(), &trace_id]), 8);
        let failed = !matches!(e.outcome, Outcome::Ok);
        spans.push(json!({
            "traceId": trace_id,
            "spanId": span_id,
            "name": e.title,
            "kind": 1,
            "startTimeUnixNano": nanos(e.start),
            "endTimeUnixNano": nanos(e.end),
            "attributes": [
                attr("sonde.operation.requests", json!(e.records.len())),
                attr("sonde.operation.kind", json!(format!("{:?}", e.kind))),
            ],
            "status": { "code": if failed { 2 } else { 1 } },
        }));
        for id in &e.records {
            let Some(r) = by_id.get(id) else { continue };
            let end = r.time + chrono::Duration::microseconds((r.latency_ms * 1000.0) as i64);
            let mut attrs = vec![
                attr("rpc.system", json!("aws-api")),
                attr("rpc.service", json!("S3")),
                attr("rpc.method", json!(r.operation)),
                attr("http.request.method", json!(r.method)),
            ];
            if let Some(st) = r.status {
                attrs.push(attr("http.response.status_code", json!(st)));
            }
            if let Some(b) = &r.bucket {
                attrs.push(attr("aws.s3.bucket", json!(b)));
            }
            if let Some(k) = &r.key {
                attrs.push(attr("aws.s3.key", json!(k)));
            }
            if let Some(q) = &r.request_id {
                attrs.push(attr("aws.request_id", json!(q)));
            }
            if let Some(n) = r.response_bytes {
                attrs.push(attr("http.response.body.size", json!(n)));
            }
            if r.retries > 0 {
                attrs.push(attr("sonde.retries", json!(r.retries)));
            }
            let error = r.is_error() && !crate::summary::is_miss(r);
            spans.push(json!({
                "traceId": trace_id,
                "spanId": hex(hash(&["req", &r.id.to_string(), &trace_id]), 8),
                "parentSpanId": span_id,
                "name": format!("S3.{}", r.operation),
                "kind": 3,
                "startTimeUnixNano": nanos(r.time),
                "endTimeUnixNano": nanos(end),
                "attributes": attrs,
                "status": { "code": if error { 2 } else { 1 } },
            }));
        }
    }
    let mut resource = vec![
        attr(
            "service.name",
            json!(
                s.header
                    .command
                    .as_deref()
                    .and_then(crate::program::program_name)
                    .unwrap_or_else(|| "sonde-session".into())
            ),
        ),
        attr("sonde.endpoint", json!(s.header.endpoint)),
    ];
    if let Some(c) = &s.header.command {
        // the variables it sets, without their values: a key never leaves
        resource.push(attr(
            "process.command_line",
            json!(crate::program::scrub_command(c)),
        ));
    }
    json!({
        "resourceSpans": [{
            "resource": { "attributes": resource },
            "scopeSpans": [{
                "scope": { "name": "sonde", "version": env!("CARGO_PKG_VERSION") },
                "spans": spans,
            }],
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionHeader;
    use chrono::DateTime;

    #[test]
    fn each_operation_is_a_span_with_its_requests_under_it() {
        let rec = |id: u64, key: &str| reqlog::Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
            method: "GET".into(),
            operation: "GetObject".into(),
            bucket: Some("b".into()),
            key: Some(key.into()),
            status: Some(200),
            latency_ms: 4.0,
            ..Default::default()
        };
        let s = Session {
            header: SessionHeader::new("e", "http://e", "us-east-1", Some("etl --day 1".into())),
            records: vec![rec(1, "a"), rec(2, "b")],
            logs: vec![],
        };
        let t = traces(&s);
        let spans = t["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .unwrap();
        let ops: Vec<&Value> = spans
            .iter()
            .filter(|s| s.get("parentSpanId").is_none())
            .collect();
        let reqs: Vec<&Value> = spans
            .iter()
            .filter(|s| s.get("parentSpanId").is_some())
            .collect();
        assert_eq!(reqs.len(), 2);
        assert!(!ops.is_empty());
        assert_eq!(reqs[0]["name"], "S3.GetObject");
        assert_eq!(reqs[0]["traceId"].as_str().unwrap().len(), 32);
        assert_eq!(reqs[0]["spanId"].as_str().unwrap().len(), 16);
        assert!(ops.iter().any(|o| o["spanId"] == reqs[0]["parentSpanId"]));
        assert_eq!(
            t["resourceSpans"][0]["resource"]["attributes"][0]["value"]["stringValue"],
            "etl"
        );
    }
}
