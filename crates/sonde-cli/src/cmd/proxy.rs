//! `sonde proxy`: a recording proxy for a program to be pointed at.

use std::io::IsTerminal;
use std::net::SocketAddr;
use std::sync::Arc;

use clap::Args;
use sonde_core::Sonde;
use sonde_proxy::Proxy;
use sonde_proxy::session::Session;

use crate::Cli;

use super::record::*;

#[derive(Args)]
pub struct ProxyArgs {
    #[command(flatten)]
    pub(super) rec: Recording,
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1:9000")]
    pub(super) listen: SocketAddr,
    /// Take requests from other machines too (any Host). Anyone who has the
    /// keys sonde prints can then use the endpoint's credentials.
    #[arg(long)]
    pub(super) allow_remote: bool,
}

pub(super) fn check_listen(a: &ProxyArgs) -> anyhow::Result<()> {
    if !a.listen.ip().is_loopback() && !a.allow_remote {
        anyhow::bail!(
            "{} is reachable from other machines: add --allow-remote if that is what you want (anyone with the keys sonde prints can then use the endpoint's credentials)",
            a.listen
        );
    }
    if a.allow_remote {
        eprintln!(
            "warning: sonde takes requests from other machines on {}",
            a.listen
        );
    }
    Ok(())
}

pub(super) fn print_env(proxy: &Proxy) {
    let (set, unset) = proxy.env();
    eprintln!(
        "sonde is listening on {}. Point the application at it:\n",
        proxy.url()
    );
    for k in &unset {
        eprintln!("  unset {k}");
    }
    for (k, v) in &set {
        eprintln!("  export {k}={v}");
    }
    eprintln!("\nCtrl-C stops.\n");
}

pub async fn proxy(s: &Arc<Sonde>, _cli: &Cli, a: &ProxyArgs) -> anyhow::Result<i32> {
    check_listen(a)?;
    let mut started = start(s, &a.rec, a.listen, None, a.allow_remote).await?;
    print_env(&started.proxy);
    let printer = follow(&started.log, false);
    tokio::signal::ctrl_c().await?;
    printer.abort();
    // requests still in flight are recorded before the file is read and packed
    shut_down(&mut started.proxy, SHUTDOWN_GRACE).await;
    started.writer.flush()?;
    let session = Session::open(&started.path).map_err(|e| anyhow::anyhow!(e))?;
    print_summary(
        &session.records,
        &session.header.endpoint,
        &started.path,
        std::io::stderr().is_terminal(),
        &request_prices_for(s, &session.header.endpoint),
        false,
    );
    // finished: compressed to a tenth or less, still a .sonde file
    let file = started.path.clone();
    drop(started);
    let _ = sonde_proxy::session::compress(&file);
    Ok(0)
}
