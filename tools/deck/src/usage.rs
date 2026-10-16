//! Reading what deck-usaged and deck-focusd recorded, merged per package.

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use deck::pkgmap::{self, PkgMap};
use rusqlite::{Connection, OpenFlags, params};
use std::collections::HashMap;
use std::path::Path;

#[derive(Default, Clone)]
pub struct Row {
    /// A package (`category/name`) or another label: `user:claude`, `kernel:kworker`, `kmod:i915`.
    pub label: String,
    pub cpu_us: i64,
    /// CPU per context (user, system, build, jail, kernel), largest first.
    pub ctx: Vec<(String, i64)>,
    pub alive_s: i64,
    pub launches: i64,
    pub rss_peak_kb: i64,
    pub pss_avg_kb: i64,
    /// Seconds in which it used more than one core.
    pub hot_s: i64,
    pub spikes: i64,
    /// Time its window had the focus with the user active.
    pub active_s: i64,
    /// Profiler samples in its code, whichever process ran it.
    pub code_samples: i64,
}

pub struct Usage {
    pub rows: Vec<Row>,
    pub total_cpu_us: i64,
    pub total_samples: i64,
    /// When deck-usaged last started, and whether deck-focusd has a database.
    pub started: Option<i64>,
    pub focus: bool,
}

pub struct Spike {
    pub start: i64,
    pub secs: i64,
    pub label: String,
    pub ctx: String,
    pub cpu_us: i64,
    pub peak_cores: f64,
    pub rss_peak_kb: i64,
}

fn open(db: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("opening {}", db.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(c)
}

fn since(days: f64) -> i64 {
    Local::now().timestamp() - (days * 86400.0) as i64
}

pub fn when(t: i64) -> String {
    Local
        .timestamp_opt(t, 0)
        .single()
        .map_or(String::new(), |t| t.format("%Y-%m-%d %H:%M").to_string())
}

pub fn load(db: &Path, days: f64) -> Result<Usage> {
    if !db.exists() {
        anyhow::bail!("{} does not exist: is deck-usaged running?", db.display());
    }
    let c = open(db)?;
    let from = since(days) / 3600 * 3600;
    let mut rows: HashMap<String, Row> = HashMap::new();
    let mut pss: HashMap<String, (i64, i64)> = HashMap::new();
    let mut q = c.prepare(
        "SELECT pkg, ctx, sum(cpu_us), sum(alive_s), sum(launches), max(rss_peak_kb), sum(pss_kb_sum),
                sum(pss_n), sum(s_200 + s_more)
         FROM usage WHERE hour >= ?1 GROUP BY pkg, ctx",
    )?;
    let mut it = q.query([from])?;
    while let Some(r) = it.next()? {
        let label: String = r.get(0)?;
        let row = rows.entry(label.clone()).or_insert_with(|| Row {
            label: label.clone(),
            ..Row::default()
        });
        let cpu: i64 = r.get(2)?;
        row.cpu_us += cpu;
        row.ctx.push((r.get(1)?, cpu));
        row.alive_s = row.alive_s.max(r.get(3)?);
        row.launches += r.get::<_, i64>(4)?;
        row.rss_peak_kb = row.rss_peak_kb.max(r.get(5)?);
        row.hot_s += r.get::<_, i64>(8)?;
        let p = pss.entry(label).or_default();
        p.0 += r.get::<_, i64>(6)?;
        p.1 += r.get::<_, i64>(7)?;
    }
    for (label, (sum, n)) in pss {
        if let Some(r) = rows.get_mut(&label) {
            r.pss_avg_kb = sum / n.max(1);
        }
    }
    let mut q = c.prepare("SELECT pkg, count(*) FROM spikes WHERE start >= ?1 GROUP BY pkg")?;
    let mut it = q.query([since(days)])?;
    while let Some(r) = it.next()? {
        let label: String = r.get(0)?;
        rows.entry(label.clone())
            .or_insert_with(|| Row {
                label,
                ..Row::default()
            })
            .spikes = r.get(1)?;
    }
    // The profiler's table is newer than the rest: an older database lacks it.
    let mut total_samples = 0;
    if let Ok(mut q) = c.prepare("SELECT lib, sum(samples) FROM libs WHERE hour >= ?1 GROUP BY lib") {
        let mut it = q.query([from])?;
        while let Some(r) = it.next()? {
            let label: String = r.get(0)?;
            let n: i64 = r.get(1)?;
            total_samples += n;
            rows.entry(label.clone())
                .or_insert_with(|| Row {
                    label,
                    ..Row::default()
                })
                .code_samples = n;
        }
    }
    let started = c
        .query_row("SELECT value FROM meta WHERE key = 'started'", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|v| v.parse().ok());

    let focus_db = deck::focus_db();
    let focus = focus_db.exists();
    if focus {
        let f = open(&focus_db)?;
        let mut q = f.prepare("SELECT program, sum(secs) FROM active WHERE hour >= ?1 GROUP BY program")?;
        let active: Vec<(String, i64)> = q
            .query_map([from], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        if !active.is_empty() {
            let map = PkgMap::build();
            for (program, secs) in active {
                let label = map
                    .lookup(&program)
                    .map_or_else(|| pkgmap::unowned(&program), str::to_string);
                rows.entry(label.clone())
                    .or_insert_with(|| Row {
                        label,
                        ..Row::default()
                    })
                    .active_s += secs;
            }
        }
    }

    let mut rows: Vec<Row> = rows.into_values().collect();
    for r in &mut rows {
        r.ctx.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    }
    rows.sort_by(|a, b| b.cpu_us.cmp(&a.cpu_us).then(a.label.cmp(&b.label)));
    let total_cpu_us = rows.iter().map(|r| r.cpu_us).sum();
    Ok(Usage {
        rows,
        total_cpu_us,
        total_samples,
        started,
        focus,
    })
}

/// Bursts of CPU, newest first; one package's when `label` is given.
pub fn spikes(db: &Path, days: f64, label: Option<&str>) -> Result<Vec<Spike>> {
    let c = open(db)?;
    let mut q = c.prepare(
        "SELECT start, secs, pkg, ctx, cpu_us, peak_cores, rss_peak_kb FROM spikes
         WHERE start >= ?1 AND (?2 IS NULL OR pkg = ?2) ORDER BY start DESC",
    )?;
    let rows = q.query_map(params![since(days), label], |r| {
        Ok(Spike {
            start: r.get(0)?,
            secs: r.get(1)?,
            label: r.get(2)?,
            ctx: r.get(3)?,
            cpu_us: r.get(4)?,
            peak_cores: r.get(5)?,
            rss_peak_kb: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// For one package: whose code its processes ran, and in whose processes its code ran, by samples.
/// Labels with their profiler samples, most first.
pub type Samples = Vec<(String, i64)>;

pub fn code(db: &Path, days: f64, label: &str) -> Result<(Samples, Samples)> {
    let c = open(db)?;
    let from = since(days) / 3600 * 3600;
    let by = |sql: &str| -> Result<Vec<(String, i64)>> {
        let Ok(mut q) = c.prepare(sql) else {
            return Ok(vec![]);
        };
        let rows = q.query_map(params![from, label], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    };
    let runs = by(
        "SELECT lib, sum(samples) s FROM libs WHERE hour >= ?1 AND prog = ?2 GROUP BY lib ORDER BY s DESC",
    )?;
    let ran_by = by(
        "SELECT prog, sum(samples) s FROM libs WHERE hour >= ?1 AND lib = ?2 GROUP BY prog ORDER BY s DESC",
    )?;
    Ok((runs, ran_by))
}

pub fn pct(part: i64, total: i64) -> f64 {
    100.0 * part as f64 / total.max(1) as f64
}

pub fn ctx_summary(r: &Row) -> String {
    r.ctx
        .iter()
        .map(|(c, us)| format!("{c}:{:.1}", *us as f64 / 1e6))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn print_totals(db: &Path, days: f64) -> Result<()> {
    let u = load(db, days)?;
    println!(
        "pkg\tcpu_s\tcpu_share\tcode_share\tactive_h\talive_h\tlaunches\trss_peak_mib\tpss_avg_mib\tsecs_over_1_core\tspikes\tctx_cpu_s"
    );
    for r in &u.rows {
        println!(
            "{}\t{:.1}\t{:.2}%\t{:.2}%\t{:.2}\t{:.1}\t{}\t{:.0}\t{:.0}\t{}\t{}\t{}",
            r.label,
            r.cpu_us as f64 / 1e6,
            pct(r.cpu_us, u.total_cpu_us),
            pct(r.code_samples, u.total_samples),
            r.active_s as f64 / 3600.0,
            r.alive_s as f64 / 3600.0,
            r.launches,
            r.rss_peak_kb as f64 / 1024.0,
            r.pss_avg_kb as f64 / 1024.0,
            r.hot_s,
            r.spikes,
            ctx_summary(r),
        );
    }
    Ok(())
}

pub fn print_spikes(db: &Path, days: f64, label: Option<&str>) -> Result<()> {
    println!("start\tsecs\tpkg\tctx\tcpu_s\tpeak_cores\trss_peak_mib");
    for s in spikes(db, days, label)?.iter().rev() {
        println!(
            "{}\t{}\t{}\t{}\t{:.1}\t{:.2}\t{:.0}",
            when(s.start),
            s.secs,
            s.label,
            s.ctx,
            s.cpu_us as f64 / 1e6,
            s.peak_cores,
            s.rss_peak_kb as f64 / 1024.0
        );
    }
    Ok(())
}

pub fn print_code(db: &Path, days: f64, label: &str) -> Result<()> {
    let (runs, ran_by) = code(db, days, label)?;
    let total: i64 = runs.iter().map(|r| r.1).sum();
    println!("# code that {label}'s processes ran");
    println!("lib\tsamples\tshare");
    for (l, n) in &runs {
        println!("{l}\t{n}\t{:.1}%", pct(*n, total));
    }
    let total: i64 = ran_by.iter().map(|r| r.1).sum();
    println!("# processes that ran {label}'s code");
    println!("prog\tsamples\tshare");
    for (p, n) in &ran_by {
        println!("{p}\t{n}\t{:.1}%", pct(*n, total));
    }
    Ok(())
}
