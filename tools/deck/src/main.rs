//! deck: what is installed on this Gentoo machine, where each package came from, which carry local
//! patches, and where each patch stands upstream.

mod model;
mod patches;
mod send;
mod track;
mod tui;
mod usage;
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
    /// Put a patch on a new branch of its upstream project's clone in ~/3p, staged and not
    /// committed, and open a terminal there
    Stage {
        id: String,
        #[arg(long)]
        no_terminal: bool,
    },
    /// In a clone: make your fork on GitHub and add it as the remote `fork`
    Fork {
        #[arg(short = 'C', default_value = ".")]
        dir: PathBuf,
    },
    /// In a clone: push the current branch to your fork
    Push {
        #[arg(short = 'C', default_value = ".")]
        dir: PathBuf,
    },
    /// In a clone: open the browser to write the pull request; once it exists, mark the patch sent
    Pr {
        #[arg(short = 'C', default_value = ".")]
        dir: PathBuf,
    },
    /// Print CPU and memory use per package, as deck-usaged recorded it, tab-separated
    Usage {
        /// How far back, in days
        #[arg(long, default_value_t = 1.0)]
        days: f64,
        /// List the bursts of CPU instead of the totals
        #[arg(long)]
        spikes: bool,
        /// One package (or label such as user:claude): whose code it ran, who ran its code, its spikes
        #[arg(long)]
        pkg: Option<String>,
        #[arg(long, env = "DECK_USAGE_DB", default_value = deck::USAGE_DB)]
        db: PathBuf,
    },
}

fn main() -> Result<()> {
    // Die quietly on a closed pipe (`deck usage | head`), as command-line tools do.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let cli = Cli::parse();
    let host_dir = match cli.host_dir {
        Some(d) => d,
        None => default_host_dir()?,
    };
    if let Some(Cmd::Usage {
        days,
        spikes,
        pkg,
        db,
    }) = &cli.cmd
    {
        return match (pkg, spikes) {
            (Some(p), _) => {
                usage::print_code(db, *days, p)?;
                println!("# spikes");
                usage::print_spikes(db, *days, Some(p))
            }
            (None, true) => usage::print_spikes(db, *days, None),
            (None, false) => usage::print_totals(db, *days),
        };
    }
    match &cli.cmd {
        Some(Cmd::Fork { dir }) => {
            let fork = send::fork(dir, true)?;
            println!("deck: your fork is {fork}, the remote fork");
            return Ok(());
        }
        Some(Cmd::Push { dir }) => return send::push(dir, true),
        _ => {}
    }
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
        Some(Cmd::Stage { id, no_terminal }) => {
            let s = m.stage(&id)?;
            if s.fresh {
                println!("deck: staged on branch {} in {}", s.branch, s.dir.display());
            } else {
                println!(
                    "deck: branch {} in {} was there already, left as it is",
                    s.branch,
                    s.dir.display()
                );
            }
            println!("deck: there: git diff --cached, git commit, then deck fork, deck push, deck pr");
            if !no_terminal {
                send::terminal(&s.dir)?;
            }
            Ok(())
        }
        Some(Cmd::Pr { dir }) => {
            let pr = send::pr(&dir, true)?;
            let Some(url) = pr.url else {
                println!("deck: once the pull request is created, run deck pr again to mark the patch sent");
                return Ok(());
            };
            let Some(row) = m.staged_on(&dir, &pr.branch) else {
                println!("deck: {url} (no local patch staged on {})", pr.branch);
                return Ok(());
            };
            let id = row.id.clone();
            match m.last_event(row).map(|e| e.state) {
                None | Some(State::Unsent) => {
                    m.record(&id, State::Sent, url.clone(), String::new())?;
                    println!("deck: {url}; {id} marked sent");
                }
                Some(s) => println!("deck: {url}; {id} is {}", s.label()),
            }
            Ok(())
        }
        Some(Cmd::Usage { .. } | Cmd::Fork { .. } | Cmd::Push { .. }) => unreachable!(),
    }
}
