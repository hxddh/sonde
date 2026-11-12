//! A `.sonde` file: its header line and its records, plain or gzipped, read
//! whole and lean (as `show` reads one).
#![no_main]

use sonde_proxy::session::{Keep, Session};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    // a session is read from a file: one per process, written over each time
    let path = std::env::temp_dir().join(format!("sonde-fuzz-{}.sonde", std::process::id()));
    if std::fs::write(&path, data).is_err() {
        return;
    }
    let _ = Session::read(&path, Keep::All);
    let _ = Session::read(&path, Keep::Analysis);
    let _ = sonde_proxy::session::read_header(&path);
});
