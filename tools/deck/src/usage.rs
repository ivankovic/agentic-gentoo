//! Reading what deck-usaged recorded.

use anyhow::{Context, Result};
use chrono::{Local, TimeZone};
use rusqlite::{Connection, OpenFlags, params};
use std::path::Path;

pub const DEFAULT_DB: &str = "/var/lib/deck/usage.db";

fn open(db: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("opening {} (is deck-usaged running?)", db.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(c)
}

fn since(days: f64) -> i64 {
    Local::now().timestamp() - (days * 86400.0) as i64
}

fn when(t: i64) -> String {
    Local
        .timestamp_opt(t, 0)
        .single()
        .map_or(String::new(), |t| t.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// Totals per package and context over the last `days`, busiest first.
pub fn print_totals(db: &Path, days: f64) -> Result<()> {
    let c = open(db)?;
    let from = since(days) / 3600 * 3600;
    let total: i64 = c.query_row(
        "SELECT coalesce(sum(cpu_us), 0) FROM usage WHERE hour >= ?1",
        [from],
        |r| r.get(0),
    )?;
    let mut q = c.prepare(
        "SELECT u.pkg, u.ctx, sum(u.cpu_us), sum(u.alive_s), sum(u.launches), max(u.rss_peak_kb),
                sum(u.pss_kb_sum) / max(sum(u.pss_n), 1), sum(u.s_200 + u.s_more),
                (SELECT count(*) FROM spikes s WHERE s.pkg = u.pkg AND s.ctx = u.ctx AND s.start >= ?2)
         FROM usage u WHERE u.hour >= ?1 GROUP BY u.pkg, u.ctx ORDER BY sum(u.cpu_us) DESC, u.pkg",
    )?;
    println!(
        "pkg\tctx\tcpu_s\tcpu_share\talive_h\tlaunches\trss_peak_mib\tpss_avg_mib\tsecs_over_1_core\tspikes"
    );
    let rows = q.query_map(params![from, since(days)], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, i64>(8)?,
        ))
    })?;
    for row in rows {
        let (pkg, ctx, cpu, alive, launches, rss, pss, hot, spikes) = row?;
        println!(
            "{pkg}\t{ctx}\t{:.1}\t{:.2}%\t{:.1}\t{launches}\t{:.0}\t{:.0}\t{hot}\t{spikes}",
            cpu as f64 / 1e6,
            100.0 * cpu as f64 / total.max(1) as f64,
            alive as f64 / 3600.0,
            rss as f64 / 1024.0,
            pss as f64 / 1024.0,
        );
    }
    Ok(())
}

/// Bursts of CPU over the last `days`, oldest first.
pub fn print_spikes(db: &Path, days: f64) -> Result<()> {
    let c = open(db)?;
    let mut q = c.prepare(
        "SELECT start, secs, pkg, ctx, cpu_us, peak_cores, rss_peak_kb FROM spikes WHERE start >= ?1 ORDER BY start",
    )?;
    println!("start\tsecs\tpkg\tctx\tcpu_s\tpeak_cores\trss_peak_mib");
    let rows = q.query_map([since(days)], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, f64>(5)?,
            r.get::<_, i64>(6)?,
        ))
    })?;
    for row in rows {
        let (start, secs, pkg, ctx, cpu, peak, rss) = row?;
        println!(
            "{}\t{secs}\t{pkg}\t{ctx}\t{:.1}\t{peak:.2}\t{:.0}",
            when(start),
            cpu as f64 / 1e6,
            rss as f64 / 1024.0
        );
    }
    Ok(())
}
