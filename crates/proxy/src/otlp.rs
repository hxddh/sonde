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
    let mut doc = Vec::new();
    // (into memory: nothing to fail)
    let _ = write_traces(s, &mut doc, false);
    serde_json::from_slice(&doc).unwrap_or(Value::Null)
}

/// The trace every span of a session belongs to.
fn trace_id(s: &Session) -> String {
    let created = s.header.created.to_rfc3339();
    let trace_hi = hash(&["sonde", &created, &s.header.endpoint]);
    let trace_lo = hash(&[&created, s.header.command.as_deref().unwrap_or("")]);
    format!("{trace_hi:016x}{trace_lo:016x}")
}

/// What the spans are of: the program (by the name it was given, else its
/// command's), the endpoint, the command line.
fn resource(s: &Session) -> Vec<Value> {
    let named = s
        .header
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(String::from);
    let mut resource = vec![
        attr(
            "service.name",
            json!(
                named
                    .or_else(|| s
                        .header
                        .command
                        .as_deref()
                        .and_then(crate::program::program_name))
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
    resource
}

/// An operation's span, then one for each of its requests under it.
fn spans_of(
    i: usize,
    e: &crate::episode::Episode,
    trace_id: &str,
    by_id: &std::collections::HashMap<u64, &reqlog::Record>,
    each: &mut dyn FnMut(Value) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let span_id = hex(hash(&["op", &i.to_string(), trace_id]), 8);
    let failed = !matches!(e.outcome, Outcome::Ok);
    each(json!({
        "traceId": trace_id,
        "spanId": span_id,
        "name": e.title(),
        "kind": 1,
        "startTimeUnixNano": nanos(e.start),
        "endTimeUnixNano": nanos(e.end),
        "attributes": [
            attr("sonde.operation.requests", json!(e.records.len())),
            attr("sonde.operation.shape", json!(format!("{:?}", e.shape))),
        ],
        "status": { "code": if failed { 2 } else { 1 } },
    }))?;
    for id in &e.records {
        let Some(r) = by_id.get(id) else { continue };
        // clamped: a corrupt latency must not overflow the date
        let end = r.time + chrono::Duration::microseconds(r.latency_us());
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
        each(json!({
            "traceId": trace_id,
            "spanId": hex(hash(&["req", &r.id.to_string(), trace_id]), 8),
            "parentSpanId": span_id,
            "name": format!("S3.{}", r.operation),
            "kind": 3,
            "startTimeUnixNano": nanos(r.time),
            "endTimeUnixNano": nanos(end),
            "attributes": attrs,
            "status": { "code": if error { 2 } else { 1 } },
        }))?;
    }
    Ok(())
}

/// A request's document around its spans: opened, and closed.
fn open_doc<W: std::io::Write>(
    out: W,
    pretty: bool,
    resource: &[Value],
) -> std::io::Result<crate::share::JsonOut<W>> {
    let mut w = crate::share::JsonOut::new(out, pretty);
    w.begin_object()?;
    w.key("resourceSpans")?;
    w.begin_array()?;
    w.begin_object()?;
    w.field("resource", &json!({ "attributes": resource }))?;
    w.key("scopeSpans")?;
    w.begin_array()?;
    w.begin_object()?;
    w.field(
        "scope",
        &json!({ "name": "sonde", "version": env!("CARGO_PKG_VERSION") }),
    )?;
    w.key("spans")?;
    w.begin_array()?;
    Ok(w)
}

fn close_doc<W: std::io::Write>(mut w: crate::share::JsonOut<W>) -> std::io::Result<W> {
    w.end_array()?;
    w.end_object()?;
    w.end_array()?;
    w.end_object()?;
    w.end_array()?;
    w.end_object()?;
    w.finish()
}

/// [`traces`], written as it is made: a span at a time, never the document
/// whole.
pub fn write_traces(s: &Session, out: impl std::io::Write, pretty: bool) -> std::io::Result<()> {
    let trace_id = trace_id(s);
    let eps = episodes(&s.records);
    let by_id: std::collections::HashMap<u64, &reqlog::Record> =
        s.records.iter().map(|r| (r.id, r)).collect();
    let mut w = open_doc(out, pretty, &resource(s))?;
    for (i, e) in eps.iter().enumerate() {
        spans_of(i, e, &trace_id, &by_id, &mut |span| w.json(&span))?;
    }
    close_doc(w)?;
    Ok(())
}

/// The session as several `ExportTraceServiceRequest`s, each under `limit`
/// bytes or so (an operation's spans stay in one), handed to `send` one at
/// a time: a collector takes a request of a few MiB, not a session's worth.
/// Returns how many were sent.
pub fn batches(
    s: &Session,
    limit: usize,
    send: &mut dyn FnMut(Vec<u8>) -> std::io::Result<()>,
) -> std::io::Result<usize> {
    let trace_id = trace_id(s);
    let resource = resource(s);
    let eps = episodes(&s.records);
    let by_id: std::collections::HashMap<u64, &reqlog::Record> =
        s.records.iter().map(|r| (r.id, r)).collect();
    let mut sent = 0;
    let mut w = open_doc(Vec::new(), false, &resource)?;
    let mut any = false;
    for (i, e) in eps.iter().enumerate() {
        spans_of(i, e, &trace_id, &by_id, &mut |span| w.json(&span))?;
        any = true;
        if w.written() >= limit {
            let full = std::mem::replace(&mut w, open_doc(Vec::new(), false, &resource)?);
            send(close_doc(full)?)?;
            sent += 1;
            any = false;
        }
    }
    // (a session without requests is still one request: its resource)
    if any || sent == 0 {
        send(close_doc(w)?)?;
        sent += 1;
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionHeader;
    use chrono::DateTime;

    #[test]
    fn traces_written_a_span_at_a_time_and_in_batches_are_the_same_spans() {
        let rec = |id: u64| reqlog::Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
            method: "GET".into(),
            operation: "GetObject".into(),
            bucket: Some("b".into()),
            key: Some(format!("k{id}").into()),
            status: Some(200),
            latency_ms: 4.0,
            ..Default::default()
        };
        let mut header = SessionHeader::new("e", "http://e", "r", Some("python etl.py".into()));
        let unnamed = Session {
            header: header.clone(),
            records: (1..=300).map(rec).collect(),
            logs: vec![],
        };
        let whole = traces(&unnamed);
        let spans = |doc: &Value| -> Vec<Value> {
            doc["resourceSpans"][0]["scopeSpans"][0]["spans"]
                .as_array()
                .unwrap()
                .clone()
        };
        assert_eq!(spans(&whole).len(), 600);
        // pretty, as `sonde share --otlp` writes it: the same document
        let mut text = Vec::new();
        write_traces(&unnamed, &mut text, true).unwrap();
        assert_eq!(
            String::from_utf8(text).unwrap(),
            serde_json::to_string_pretty(&whole).unwrap()
        );
        // in requests of 8 KiB or so: every span once, in order, each
        // request a whole document with the resource
        let mut parts: Vec<Vec<u8>> = Vec::new();
        let n = batches(&unnamed, 8 << 10, &mut |b| {
            parts.push(b);
            Ok(())
        })
        .unwrap();
        assert!(n > 5 && n == parts.len(), "{n} parts");
        let mut all = Vec::new();
        for p in &parts {
            assert!(p.len() < 12 << 10, "{} bytes", p.len());
            let doc: Value = serde_json::from_slice(p).unwrap();
            assert_eq!(
                doc["resourceSpans"][0]["resource"],
                whole["resourceSpans"][0]["resource"]
            );
            all.extend(spans(&doc));
        }
        assert_eq!(all, spans(&whole));
        // the program's name, when it has one, is the service
        let service =
            |s: &Session| traces(s)["resourceSpans"][0]["resource"]["attributes"][0].clone();
        assert_eq!(service(&unnamed)["value"]["stringValue"], "python");
        header.name = Some("nightly-etl".into());
        let named = Session {
            header,
            records: vec![rec(1)],
            logs: vec![],
        };
        assert_eq!(service(&named)["key"], "service.name");
        assert_eq!(service(&named)["value"]["stringValue"], "nightly-etl");
    }

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

    #[test]
    fn a_latency_no_request_could_have_does_not_overflow_the_date() {
        let s = Session {
            header: SessionHeader::new("e", "http://e", "us-east-1", None),
            records: vec![reqlog::Record {
                id: 1,
                time: DateTime::from_timestamp_millis(1_700_000_000_000).unwrap(),
                method: "GET".into(),
                operation: "GetObject".into(),
                status: Some(200),
                latency_ms: 1e300,
                ..Default::default()
            }],
            logs: vec![],
        };
        let t = traces(&s);
        let spans = t["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .unwrap();
        assert!(spans.iter().any(|s| s["name"] == "S3.GetObject"));
    }
}
