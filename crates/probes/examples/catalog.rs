//! The probe catalog as JSON, for `scripts/gen-probe-docs.py`:
//!
//!     cargo run -q -p probes --example catalog
fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(&probes::catalog()).expect("the catalog is JSON")
    );
}
