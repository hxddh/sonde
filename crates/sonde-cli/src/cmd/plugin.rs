//! `sonde plugin`: signed plugin packages (terminal builds).

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Subcommand};
use sonde_core::Sonde;
use sonde_nsh::plugins::Plugins;

use crate::Cli;
use crate::output;

#[derive(Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    action: PluginAction,
}

#[derive(Subcommand)]
enum PluginAction {
    /// Installed plugins and trusted roots.
    List,
    /// Trusts a publisher's root key (`root.pub`).
    Trust {
        label: String,
        key: PathBuf,
    },
    /// Stops trusting a root (its plugins are disabled).
    Untrust {
        id: String,
    },
    /// Verifies and installs a `.nshpkg`.
    Install {
        package: PathBuf,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    Remove {
        id: String,
    },
    /// Applies a signed revocation list (`nsh revoke`).
    Revoke {
        list: PathBuf,
    },
}

pub async fn run(s: &Arc<Sonde>, _cli: &Cli, a: &PluginArgs) -> anyhow::Result<i32> {
    let p = Plugins::open(s);
    let e = |m: String| anyhow::anyhow!(m);
    match &a.action {
        PluginAction::List => {
            println!(
                "{}",
                output::table(
                    &[
                        "id", "name", "version", "build", "enabled", "revoked", "signer"
                    ],
                    &p.list()
                        .iter()
                        .map(|x| vec![
                            x.id.clone(),
                            x.name.clone(),
                            x.version.clone(),
                            x.build.to_string(),
                            x.enabled.to_string(),
                            x.revoked.to_string(),
                            x.key_id.clone()
                        ])
                        .collect::<Vec<_>>()
                )
            );
            println!(
                "\n{}",
                output::table(
                    &["root", "label", "added"],
                    &p.roots()
                        .iter()
                        .map(|r| vec![r.id.clone(), r.label.clone(), r.added.clone()])
                        .collect::<Vec<_>>()
                )
            );
        }
        PluginAction::Trust { label, key } => {
            let r = p
                .add_root(label, std::fs::read_to_string(key)?.trim())
                .map_err(e)?;
            println!("trusted root {} ({})", r.id, r.label);
        }
        PluginAction::Untrust { id } => p.remove_root(id).map_err(e)?,
        PluginAction::Install { package } => {
            let i = p.install(package).map_err(e)?;
            println!(
                "installed {} {} (build {}), signed by {}, host calls: {}",
                i.id,
                i.version,
                i.build,
                i.key_id,
                i.host_calls.join(", ")
            );
        }
        PluginAction::Enable { id } => p.set_enabled(id, true).map_err(e)?,
        PluginAction::Disable { id } => p.set_enabled(id, false).map_err(e)?,
        PluginAction::Remove { id } => p.uninstall(id).map_err(e)?,
        PluginAction::Revoke { list } => println!("{}", p.apply_revocations(list).map_err(e)?),
    }
    Ok(0)
}
