//! deck: what is installed on this Gentoo machine, where each package came from, which carry local
//! patches, and where each patch stands upstream.

mod model;
mod patches;
mod review;
mod scan;
mod send;
mod track;
mod tui;
mod usage;
mod vdb;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use model::{Model, date, default_host_dir};
use review::Kind;
use std::path::PathBuf;
use track::State;
use vdb::Filter;

#[derive(Parser)]
#[command(
    version,
    about = "Installed packages, their origin, local patches and their way upstream"
)]
struct Cli {
    /// This host's directory [default: ~/.config/deck/host-dir's, else ~/src/agentic-gentoo/hosts/<hostname>]
    #[arg(long, env = "DECK_HOST_DIR", global = true)]
    host_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Security and performance reviews by AI agents, as review.toml lists them [default: list]
    Review {
        #[command(subcommand)]
        cmd: Option<ReviewCmd>,
    },
    /// Scans for malicious code in what an emerge would install, and the gate the emerge wrapper
    /// asks before installing
    Scan {
        #[command(subcommand)]
        cmd: ScanCmd,
    },
    /// Print installed packages, tab-separated
    Packages {
        #[arg(long, value_enum, default_value_t = Filter::All)]
        filter: Filter,
    },
    /// Print local patches and their upstream state, tab-separated
    Patches,
    /// Record a change of a patch's upstream state, timestamped now
    Mark {
        /// The patch, as `deck patches patches` prints it: category/package/file
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

#[derive(Subcommand)]
enum ReviewCmd {
    /// Every subject and where its reviews stand, the most worthwhile next first, tab-separated
    List,
    /// The most worthwhile review to run next, and why
    Next {
        #[arg(long, value_enum)]
        kind: Option<Kind>,
    },
    /// Run one review now: the difference from the last reviewed version, or the whole
    Run {
        /// ours/<name> or category/package, as deck patches review list prints it
        subject: String,
        #[arg(long, value_enum, default_value = "security")]
        kind: Kind,
        /// One of review.toml's agents [default: its default]
        #[arg(long)]
        agent: Option<String>,
        /// Review the whole, even when an earlier version was reviewed
        #[arg(long)]
        full: bool,
    },
    /// Security reviews of updates to watched code, within the daily budget (the timer runs it)
    Auto,
    /// Fetch and unpack a subject's source as a review would read it, without an agent
    Prepare {
        subject: String,
        /// A version other than the one a review would take
        #[arg(long)]
        version: Option<String>,
    },
    /// Have an agent check whether the subject's open findings are fixed in the installed version
    Recheck {
        subject: String,
        #[arg(long)]
        agent: Option<String>,
    },
    /// Findings and what was decided about them, open ones first, tab-separated
    Findings {
        /// ours/<name> or category/package [default: every subject]
        subject: Option<String>,
        /// Also the decided ones
        #[arg(long)]
        all: bool,
    },
    /// Decide about a finding (the next review is told): open, fixed, not-applicable, wont-fix
    Decide {
        /// The run, as deck patches review findings prints it
        run: String,
        /// The finding's number in that run, as deck patches review findings prints it
        index: usize,
        decision: String,
        #[arg(long, default_value = "")]
        note: String,
    },
}

#[derive(Subcommand)]
enum ScanCmd {
    /// Scan what `emerge --update --deep --newuse @world` would install (the timer runs it)
    Pending {
        /// One of review.toml's agents [default: its [scan] agent, else its default]
        #[arg(long)]
        agent: Option<String>,
    },
    /// Scan these versions (category/package-version) now, again if they were scanned before
    Run {
        #[arg(required = true)]
        cpvs: Vec<String>,
        #[arg(long)]
        agent: Option<String>,
    },
    /// For the emerge wrapper: scan what emerge would install with these arguments, then exit 1
    /// when anything may not be installed
    Gate {
        #[arg(long)]
        agent: Option<String>,
        /// emerge's arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        args: Vec<String>,
    },
    /// Every verdict, newest last, tab-separated
    List,
    /// Let a version be installed after a look at its scan
    Allow {
        /// category/package-version
        cpv: String,
        #[arg(long, default_value = "")]
        note: String,
    },
}

fn main() -> Result<()> {
    // Die quietly on a closed pipe (`deck patches usage | head`), as command-line tools do.
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
    if let Some(Cmd::Review { cmd }) = &cli.cmd {
        return review_cmd(&host_dir, cmd.as_ref().unwrap_or(&ReviewCmd::List));
    }
    if let Some(Cmd::Scan { cmd }) = &cli.cmd {
        return scan_cmd(&host_dir, cmd);
    }
    match &cli.cmd {
        Some(Cmd::Fork { dir }) => {
            let fork = send::fork(dir, true)?;
            println!("deck-patches: your fork is {fork}, the remote fork");
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
                println!("deck-patches: staged on branch {} in {}", s.branch, s.dir.display());
            } else {
                println!(
                    "deck-patches: branch {} in {} was there already, left as it is",
                    s.branch,
                    s.dir.display()
                );
            }
            println!("deck-patches: there: git diff --cached, git commit, then deck patches fork, deck patches push, deck patches pr");
            if !no_terminal {
                send::terminal(&s.dir)?;
            }
            Ok(())
        }
        Some(Cmd::Pr { dir }) => {
            let pr = send::pr(&dir, true)?;
            let Some(url) = pr.url else {
                println!("deck-patches: once the pull request is created, run deck patches pr again to mark the patch sent");
                return Ok(());
            };
            let Some(row) = m.staged_on(&dir, &pr.branch) else {
                println!("deck-patches: {url} (no local patch staged on {})", pr.branch);
                return Ok(());
            };
            let id = row.id.clone();
            match m.last_event(row).map(|e| e.state) {
                None | Some(State::Unsent | State::Reported) => {
                    m.record(&id, State::Sent, url.clone(), String::new())?;
                    println!("deck-patches: {url}; {id} marked sent");
                }
                Some(s) => println!("deck-patches: {url}; {id} is {}", s.label()),
            }
            Ok(())
        }
        Some(Cmd::Usage { .. } | Cmd::Fork { .. } | Cmd::Push { .. } | Cmd::Review { .. } | Cmd::Scan { .. }) => unreachable!(),
    }
}

/// The repository a host directory is in: `<repo>/hosts/<host>`.
fn repo_of(host_dir: &std::path::Path) -> PathBuf {
    host_dir.ancestors().nth(2).map_or_else(|| host_dir.to_path_buf(), PathBuf::from)
}

fn review_cmd(host_dir: &std::path::Path, cmd: &ReviewCmd) -> Result<()> {
    let cfg = review::Config::load(host_dir)?;
    let repo = repo_of(host_dir);
    let usage_db = std::env::var_os("DECK_USAGE_DB").map_or_else(|| PathBuf::from(deck::USAGE_DB), PathBuf::from);
    let usage = usage::load(&usage_db, 7.0).ok();
    let board = || -> Result<Vec<review::Item>> {
        Ok(review::board(&cfg, &repo, &vdb::load()?, usage.as_ref()))
    };
    match cmd {
        ReviewCmd::List => {
            let all = review::runs();
            println!("score\tsubject\ttier\tinstalled\tupdate\tsecurity\tperformance\tcpu%");
            for i in board()? {
                let name = i.subject.name();
                let (sec, _) = review::latest(&all, &name, Kind::Security);
                let (perf, _) = review::latest(&all, &name, Kind::Performance);
                println!(
                    "{:.0}\t{name}\t{}\t{}\t{}\t{}\t{}\t{:.1}",
                    i.score(),
                    i.subject.tier().map_or(String::new(), |t| t.to_string()),
                    i.versions.installed.clone().unwrap_or_default(),
                    i.versions.update.clone().unwrap_or_default(),
                    review::need_label(&i.security, sec),
                    review::need_label(&i.performance, perf),
                    i.cpu_pct,
                );
            }
            Ok(())
        }
        ReviewCmd::Next { kind } => {
            let items = board()?;
            let best = items
                .iter()
                .map(|i| match kind {
                    Some(Kind::Security) => (i, Kind::Security, i.security_score),
                    Some(Kind::Performance) => (i, Kind::Performance, i.performance_score),
                    None => (i, i.best_kind(), i.score()),
                })
                // The first of equals: the board's order breaks ties.
                .fold(None, |best: Option<(&review::Item, Kind, f64)>, x| match best {
                    Some(b) if b.2 >= x.2 => Some(b),
                    _ => Some(x),
                });
            match best {
                Some((i, k, s)) if s > 0.0 => {
                    let need = if k == Kind::Security { &i.security } else { &i.performance };
                    let what = match need {
                        review::Need::Delta { from, to } => format!("the change {from} → {to}"),
                        _ => "the whole of it, a baseline".into(),
                    };
                    println!("{} review of {}: {what} (score {s:.0})", k.label(), i.subject.name());
                    println!("deck patches review run {} --kind {}", i.subject.name(), k.label());
                }
                _ => println!("nothing to review"),
            }
            Ok(())
        }
        ReviewCmd::Run { subject, kind, agent, full } => {
            let items = board()?;
            let item = items
                .iter()
                .find(|i| i.subject.name() == *subject)
                .with_context(|| format!("no subject {subject}; deck patches review list shows them"))?;
            run_review(&cfg, &repo, item, *kind, agent.as_deref(), *full, false, false, &usage_db)
        }
        ReviewCmd::Prepare { subject, version } => {
            let items = board()?;
            let item = items
                .iter()
                .find(|i| i.subject.name() == *subject)
                .with_context(|| format!("no subject {subject}; deck patches review list shows them"))?;
            let v = match version {
                Some(v) => v.clone(),
                None => item.versions.target().context("no version")?.to_string(),
            };
            let dir = review::prepare(&repo, &item.subject, &v, &item.versions, &mut std::io::stderr())?;
            let unpatched = dir.with_file_name(format!("{v}.unpatched")).exists();
            println!("{}{}", dir.display(), if unpatched { " (upstream's source: Gentoo's patches not applied)" } else { "" });
            Ok(())
        }
        ReviewCmd::Recheck { subject, agent } => {
            let items = board()?;
            let item = items
                .iter()
                .find(|i| i.subject.name() == *subject)
                .with_context(|| format!("no subject {subject}; deck patches review list shows them"))?;
            run_review(&cfg, &repo, item, Kind::Security, agent.as_deref(), false, false, true, &usage_db)
        }
        ReviewCmd::Findings { subject, all } => {
            let runs = review::runs();
            let mut subjects: Vec<String> = runs.iter().map(|r| r.subject.clone()).collect();
            subjects.sort();
            subjects.dedup();
            println!("subject\trun\tindex\tkind\tseverity\tstatus\tversion\ttitle\twhere\tnote");
            for name in subjects.iter().filter(|n| subject.as_ref().is_none_or(|s| s == *n)) {
                for l in review::findings(&runs, name, *all) {
                    println!(
                        "{name}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        l.run.id,
                        l.index,
                        l.run.kind.label(),
                        l.finding.severity,
                        l.finding.status_label(),
                        l.run.version,
                        l.finding.title,
                        l.finding.place,
                        l.finding.note
                    );
                }
            }
            Ok(())
        }
        ReviewCmd::Decide { run, index, decision, note } => review::decide(run, *index, decision, note, ""),
        ReviewCmd::Auto => {
            let today = chrono::Local::now().date_naive();
            let done_today = review::runs()
                .iter()
                .filter(|r| r.auto)
                .filter(|r| {
                    chrono::DateTime::from_timestamp(r.started, 0)
                        .is_some_and(|t| t.with_timezone(&chrono::Local).date_naive() == today)
                })
                .count();
            let mut left = cfg.budget.auto_per_day.saturating_sub(done_today);
            for i in board()? {
                if left == 0 {
                    break;
                }
                if i.subject.tier().is_some() && matches!(i.security, review::Need::Delta { .. }) {
                    run_review(&cfg, &repo, &i, Kind::Security, None, false, true, false, &usage_db)?;
                    left -= 1;
                }
            }
            Ok(())
        }
    }
}

fn scan_cmd(host_dir: &std::path::Path, cmd: &ScanCmd) -> Result<()> {
    let cfg = review::Config::load(host_dir)?;
    let agent = |name: &Option<String>| cfg.agent(name.as_deref().or(cfg.scan.agent.as_deref()));
    let mut log = std::io::stderr();
    let print = |d: &[scan::Decision]| {
        for x in d {
            println!("{}\t{}\t{}\t{}", if x.pass { "ok" } else { "BLOCKED" }, x.label, x.cpv, x.why);
        }
    };
    match cmd {
        ScanCmd::Pending { agent: a } => {
            let wanted = scan::pending(&["--update", "--deep", "--newuse", "@world"].map(String::from))?;
            let d = scan::gate(&cfg, agent(a)?, &scan::resolve(&wanted)?, false, &mut log);
            print(&d);
            Ok(())
        }
        ScanCmd::Run { cpvs, agent: a } => {
            let d = scan::gate(&cfg, agent(a)?, &scan::resolve(cpvs)?, true, &mut log);
            print(&d);
            Ok(())
        }
        ScanCmd::Gate { agent: a, args } => {
            let wanted = scan::pending(args)?;
            let d = scan::gate(&cfg, agent(a)?, &scan::resolve(&wanted)?, false, &mut log);
            print(&d);
            let blocked = d.iter().filter(|x| !x.pass).count();
            if blocked > 0 {
                eprintln!(
                    "deck-patches: {blocked} of {} blocked. Read the reports (deck patches scan list), then \
                     `deck patches scan allow <cpv> --note ...` for each you accept.",
                    d.len()
                );
                std::process::exit(1);
            }
            Ok(())
        }
        ScanCmd::List => {
            println!("when\tstatus\tcpv\tfrom\tsignals\tbuild_files\tagent\twhy\treport\tnote");
            for v in scan::verdicts() {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    date(v.when),
                    v.status.label(),
                    v.cpv,
                    v.from.unwrap_or_default(),
                    v.signals,
                    v.build_files,
                    v.agent,
                    v.why,
                    v.report.map(|r| r.display().to_string()).unwrap_or_default(),
                    v.note
                );
            }
            Ok(())
        }
        ScanCmd::Allow { cpv, note } => scan::allow(cpv, note),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_review(
    cfg: &review::Config,
    repo: &std::path::Path,
    item: &review::Item,
    kind: Kind,
    agent: Option<&str>,
    full: bool,
    auto: bool,
    recheck: bool,
    usage_db: &std::path::Path,
) -> Result<()> {
    let agent = cfg.agent(agent)?;
    let usage = match (kind, item.subject.usage_label()) {
        (Kind::Performance, Some(l)) => review::usage_text(usage_db, l, 7.0),
        _ => String::new(),
    };
    let req = review::Request {
        subject: &item.subject,
        kind,
        agent,
        full,
        auto,
        usage,
        recheck,
    };
    let r = review::run(repo, req, &item.versions, &mut std::io::stderr())?;
    match r.status.as_str() {
        "done" if recheck => {
            let open = review::counts_label(review::open_counts(&review::runs(), &r.subject), "none");
            println!("{}: rechecked; open now: {open} · {}", r.subject, r.report.display());
            Ok(())
        }
        "done" => {
            println!("{}: {} · {}", r.subject, r.counts_label(), r.report.display());
            Ok(())
        }
        _ => anyhow::bail!("{}: {}", r.subject, r.error),
    }
}
