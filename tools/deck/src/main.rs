//! deck: what is installed on this Gentoo machine, where each package came from, which carry local
//! patches, and where each patch stands upstream.

mod model;
mod patches;
mod track;
mod tui;
mod vdb;

use anyhow::Result;
use clap::{Parser, Subcommand};
use model::{Model, date, default_host_dir};
use std::path::PathBuf;
use track::State;
use vdb::Filter;

#[derive(Parser)]
#[command(
    version,
    about = "Installed packages, their origin, local patches and their way upstream"
)]
struct Cli {
    /// This host's directory in agentic-gentoo [default: ~/src/agentic-gentoo/hosts/<hostname>]
    #[arg(long, env = "DECK_HOST_DIR", global = true)]
    host_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print installed packages, tab-separated
    Packages {
        #[arg(long, value_enum, default_value_t = Filter::All)]
        filter: Filter,
    },
    /// Print local patches and their upstream state, tab-separated
    Patches,
    /// Record a change of a patch's upstream state, timestamped now
    Mark {
        /// The patch, as `deck patches` prints it: category/package/file
        id: String,
        #[arg(value_enum)]
        state: State,
        /// Where it happened: a PR, mailing-list post, commit or bug
        #[arg(long, default_value = "")]
        url: String,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Set the upstream project a patch is meant for
    Upstream { id: String, url: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let host_dir = match cli.host_dir {
        Some(d) => d,
        None => default_host_dir()?,
    };
    let mut m = Model::load(host_dir)?;
    match cli.cmd {
        None => tui::run(m),
        Some(Cmd::Packages { filter }) => {
            println!("atom\torigin\trepo\teco\tworld\tpatches\tbuilt");
            for (i, p) in m.pkgs.iter().enumerate() {
                if filter.keep(p, m.patch_counts[i] > 0) {
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        p.atom(),
                        p.origin.label(),
                        p.repo,
                        p.eco,
                        if p.world { "yes" } else { "no" },
                        m.patch_counts[i],
                        date(p.build_time),
                    );
                }
            }
            Ok(())
        }
        Some(Cmd::Patches) => {
            println!("id\tstate\tsince\tapplies\tdeployed\tupstream\tfile");
            for r in &m.rows {
                let file = r.file.map(|i| &m.files[i]);
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    r.id,
                    m.state_label(r),
                    m.last_event(r).map_or("", |e| e.at.as_str()),
                    m.applies_label(r),
                    match file {
                        Some(f) if f.deployed => "yes",
                        Some(_) => "no",
                        None => "",
                    },
                    m.tracked(r).map_or("", |t| t.upstream.as_str()),
                    file.map_or("", |f| f.rel.as_str()),
                );
            }
            Ok(())
        }
        Some(Cmd::Mark { id, state, url, note }) => m.record(&id, state, url, note),
        Some(Cmd::Upstream { id, url }) => m.set_upstream(&id, url),
    }
}
