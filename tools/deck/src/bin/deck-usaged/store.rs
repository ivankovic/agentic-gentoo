//! Hourly totals and spikes in SQLite. The rollback journal (not WAL) lets `deck usage` read the
//! file without write access to its directory.

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::Path;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS usage (
    hour        INTEGER NOT NULL,  -- unix time of the hour's start
    pkg         TEXT    NOT NULL,  -- category/package, or kernel:<thread>, user:<app>, local:<file>, …
    ctx         TEXT    NOT NULL,  -- who it ran for: user, system, build, jail, kernel, other
    cpu_us      INTEGER NOT NULL DEFAULT 0,
    alive_s     INTEGER NOT NULL DEFAULT 0,  -- seconds with at least one process running
    launches    INTEGER NOT NULL DEFAULT 0,  -- execs
    rss_peak_kb INTEGER NOT NULL DEFAULT 0,  -- highest summed peak RSS over a 10 s window
    pss_kb_sum  INTEGER NOT NULL DEFAULT 0,  -- PSS summed over the package, added up per reading
    pss_n       INTEGER NOT NULL DEFAULT 0,  -- readings in pss_kb_sum
    -- seconds by CPU used in that second: up to 5% of a core, 25%, 1 core, 2 cores, more
    s_5 INTEGER NOT NULL DEFAULT 0, s_25 INTEGER NOT NULL DEFAULT 0, s_100 INTEGER NOT NULL DEFAULT 0,
    s_200 INTEGER NOT NULL DEFAULT 0, s_more INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (hour, pkg, ctx)
);
CREATE TABLE IF NOT EXISTS spikes (
    start       INTEGER NOT NULL,  -- unix time
    secs        INTEGER NOT NULL,
    pkg         TEXT    NOT NULL,
    ctx         TEXT    NOT NULL,
    cpu_us      INTEGER NOT NULL,
    peak_cores  REAL    NOT NULL,
    rss_peak_kb INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS spikes_start ON spikes (start);
-- The profiler: samples of where code ran, 49 a second per busy CPU, counted in cycles.
CREATE TABLE IF NOT EXISTS libs (
    hour    INTEGER NOT NULL,
    lib     TEXT    NOT NULL,  -- whose code ran: a library's or program's package, kernel, kmod:<name>
    prog    TEXT    NOT NULL,  -- the process's package
    ctx     TEXT    NOT NULL,
    samples INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (hour, lib, prog, ctx)
);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

#[derive(Default, Clone)]
pub struct Agg {
    pub cpu_us: u64,
    pub alive_s: u64,
    pub launches: u64,
    pub rss_peak_kb: u64,
    pub pss_kb_sum: u64,
    pub pss_n: u64,
    pub secs: [u64; 5],
}

pub struct Spike {
    pub start: i64,
    pub secs: i64,
    pub pkg: String,
    pub ctx: &'static str,
    pub cpu_us: u64,
    pub peak_cores: f64,
    pub rss_peak_kb: u64,
}

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch(SCHEMA)?;
        Ok(Store { db })
    }

    pub fn meta(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Adds the totals gathered since the last flush.
    pub fn flush<'a>(
        &mut self,
        rows: impl Iterator<Item = (i64, &'a str, &'a str, &'a Agg)>,
        libs: impl Iterator<Item = (i64, &'a str, &'a str, &'a str, u64)>,
        spikes: &[Spike],
    ) -> Result<()> {
        let tx = self.db.transaction()?;
        {
            let mut up = tx.prepare_cached(
                "INSERT INTO usage (hour, pkg, ctx, cpu_us, alive_s, launches, rss_peak_kb, pss_kb_sum, pss_n,
                                    s_5, s_25, s_100, s_200, s_more)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT (hour, pkg, ctx) DO UPDATE SET
                    cpu_us = cpu_us + excluded.cpu_us, alive_s = alive_s + excluded.alive_s,
                    launches = launches + excluded.launches,
                    rss_peak_kb = max(rss_peak_kb, excluded.rss_peak_kb),
                    pss_kb_sum = pss_kb_sum + excluded.pss_kb_sum, pss_n = pss_n + excluded.pss_n,
                    s_5 = s_5 + excluded.s_5, s_25 = s_25 + excluded.s_25, s_100 = s_100 + excluded.s_100,
                    s_200 = s_200 + excluded.s_200, s_more = s_more + excluded.s_more",
            )?;
            for (hour, pkg, ctx, a) in rows {
                let s = a.secs.map(|v| v as i64);
                up.execute(params![
                    hour,
                    pkg,
                    ctx,
                    a.cpu_us as i64,
                    a.alive_s as i64,
                    a.launches as i64,
                    a.rss_peak_kb as i64,
                    a.pss_kb_sum as i64,
                    a.pss_n as i64,
                    s[0],
                    s[1],
                    s[2],
                    s[3],
                    s[4]
                ])?;
            }
            let mut lib = tx.prepare_cached(
                "INSERT INTO libs (hour, lib, prog, ctx, samples) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (hour, lib, prog, ctx) DO UPDATE SET samples = samples + excluded.samples",
            )?;
            for (hour, l, prog, ctx, n) in libs {
                lib.execute(params![hour, l, prog, ctx, n as i64])?;
            }
            let mut ins = tx.prepare_cached(
                "INSERT INTO spikes (start, secs, pkg, ctx, cpu_us, peak_cores, rss_peak_kb)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for s in spikes {
                ins.execute(params![
                    s.start,
                    s.secs,
                    s.pkg,
                    s.ctx,
                    s.cpu_us as i64,
                    s.peak_cores,
                    s.rss_peak_kb as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}
