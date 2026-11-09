use std::sync::{Arc, OnceLock};

use serde::Serialize;
use serde_json::{Value, json};
use sonde_core::{Paths, S3Error, Sonde};

static SONDE: OnceLock<Arc<Sonde>> = OnceLock::new();

/// The process's sonde: opened from the platform's paths on first use
/// (`SONDE_CONFIG_DIR` / `SONDE_DATA_DIR` override them).
pub fn sonde() -> Arc<Sonde> {
    SONDE
        .get_or_init(|| {
            Sonde::open(Paths::from_env()).unwrap_or_else(|e| {
                eprintln!("sonde: configuration: {e}; starting with an empty one");
                Sonde::with_config(Paths::from_env(), Default::default())
            })
        })
        .clone()
}

/// Uses `s` as the process's sonde (hosts and tests that open it themselves).
pub fn install(s: Arc<Sonde>) -> bool {
    SONDE.set(s).is_ok()
}

/// An error as TSX reads it: JSON in the message.
pub fn err(e: S3Error) -> String {
    json!({
        "sonde": true,
        "kind": e.kind,
        "message": e.to_string(),
        "code": e.code,
        "status": e.status,
        "requestId": e.request_id,
        "recordId": e.record_id,
    })
    .to_string()
}

pub fn err_msg(m: impl std::fmt::Display) -> String {
    json!({ "sonde": true, "kind": "config", "message": m.to_string() }).to_string()
}

pub fn to_json<T: Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Runs `f` on the runtime's threads; its value as JSON.
pub async fn run<F, T>(f: F) -> Result<Value, String>
where
    F: std::future::Future<Output = Result<T, S3Error>> + Send + 'static,
    T: Serialize + Send + 'static,
{
    match nsh_core::prelude::background(f).await {
        Ok(Ok(v)) => Ok(to_json(v)),
        Ok(Err(e)) => Err(err(e)),
        Err(e) => Err(err_msg(format!("task failed: {e}"))),
    }
}

/// The request prices the settings give, for a session's cost.
pub fn request_prices() -> sonde_proxy::profile::RequestPrices {
    let p = sonde().settings().prices;
    sonde_proxy::profile::RequestPrices {
        per_1000_writes: p.per_1000_writes,
        per_1000_reads: p.per_1000_reads,
        currency: p.currency,
    }
}

/// The prices for a session on `endpoint`: the settings', unless they are
/// still S3 Standard's and the endpoint's vendor charges otherwise (R2, B2,
/// Wasabi, a store of one's own).
pub fn request_prices_for(endpoint: &str) -> sonde_proxy::profile::RequestPrices {
    let mut p = request_prices();
    let standard = sonde_proxy::profile::RequestPrices::default();
    if p.per_1000_writes == standard.per_1000_writes
        && p.per_1000_reads == standard.per_1000_reads
        && p.currency == standard.currency
        && let Some((w, r)) = sonde()
            .endpoint_config(endpoint)
            .and_then(|e| e.vendor.request_prices())
    {
        p.per_1000_writes = w;
        p.per_1000_reads = r;
    }
    p
}
