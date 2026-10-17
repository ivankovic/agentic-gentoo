//! Everything deck shows, loaded once and reloaded on request.

use crate::patches::{self, PatchFile};
use crate::send;
use crate::track::{Event, State, Tracked, Tracking};
use crate::vdb::{self, Pkg};
use anyhow::{Context, Result, bail};
use chrono::{Local, TimeZone};
use std::fs;
use std::path::{Path, PathBuf};

/// A patch as deck lists it: a file in the host's directory, an entry in upstream.toml, or both.
/// An entry without a file is a patch no longer carried, kept for its history.
pub struct PatchRow {
    pub id: String,
    pub file: Option<usize>,
    pub tracked: Option<usize>,
}

pub struct Model {
    pub host_dir: PathBuf,
    pub track_path: PathBuf,
    pub pkgs: Vec<Pkg>,
    pub files: Vec<PatchFile>,
    pub tracking: Tracking,
    pub rows: Vec<PatchRow>,
    /// Per package: how many local patches apply to it.
    pub patch_counts: Vec<usize>,
}

pub fn now() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

pub fn date(epoch: i64) -> String {
    match Local.timestamp_opt(epoch, 0).single() {
        Some(t) if epoch > 0 => t.format("%Y-%m-%d").to_string(),
        _ => String::new(),
    }
}

pub fn default_host_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let host = fs::read_to_string("/etc/hostname").context("reading /etc/hostname")?;
    Ok(PathBuf::from(home)
        .join("src/agentic-gentoo/hosts")
        .join(host.trim()))
}

impl Model {
    pub fn load(host_dir: PathBuf) -> Result<Model> {
        if !host_dir.is_dir() {
            bail!(
                "{} is not a directory; pass --host-dir or set DECK_HOST_DIR",
                host_dir.display()
            );
        }
        let track_path = host_dir.join("upstream.toml");
        let pkgs = vdb::load()?;
        let files = patches::scan(&host_dir, &pkgs);
        let tracking = Tracking::load(&track_path)?;
        let mut patch_counts = vec![0; pkgs.len()];
        for f in &files {
            if let Some(i) = f.applies {
                patch_counts[i] += 1;
            }
        }
        let mut m = Model {
            host_dir,
            track_path,
            pkgs,
            files,
            tracking,
            rows: vec![],
            patch_counts,
        };
        m.rebuild_rows();
        Ok(m)
    }

    fn rebuild_rows(&mut self) {
        let mut rows: Vec<PatchRow> = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| PatchRow {
                id: f.id.clone(),
                file: Some(i),
                tracked: self.tracking.patches.iter().position(|t| t.id == f.id),
            })
            .collect();
        for (i, t) in self.tracking.patches.iter().enumerate() {
            if !rows.iter().any(|r| r.id == t.id) {
                rows.push(PatchRow {
                    id: t.id.clone(),
                    file: None,
                    tracked: Some(i),
                });
            }
        }
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        self.rows = rows;
    }

    pub fn tracked(&self, row: &PatchRow) -> Option<&Tracked> {
        row.tracked.map(|i| &self.tracking.patches[i])
    }

    pub fn last_event(&self, row: &PatchRow) -> Option<&Event> {
        self.tracked(row).and_then(Tracked::last)
    }

    pub fn state_label(&self, row: &PatchRow) -> &'static str {
        self.last_event(row).map_or("untracked", |e| e.state.label())
    }

    /// Which installed package the patch applies to, or why none.
    pub fn applies_label(&self, row: &PatchRow) -> String {
        match row.file.map(|i| &self.files[i]) {
            None => "(file gone)".into(),
            Some(f) => match f.applies {
                Some(p) => self.pkgs[p].atom(),
                None => "(no match)".into(),
            },
        }
    }

    /// Stages the patch's file on a branch of its upstream project's clone (`send::stage`).
    pub fn stage(&self, id: &str) -> Result<send::Staged> {
        self.known(id)?;
        let row = self.rows.iter().find(|r| r.id == id).context("no such patch")?;
        let file = row
            .file
            .map(|i| &self.files[i])
            .with_context(|| format!("{id}: its file is gone"))?;
        let upstream = self
            .tracked(row)
            .map(|t| t.upstream.as_str())
            .filter(|u| !u.is_empty())
            .with_context(|| format!("{id} has no upstream project: deck upstream {id} <URL>"))?;
        send::stage(upstream, id, &self.host_dir.join(&file.rel))
    }

    /// The patch that `stage` puts on `branch` in the clone at `dir`, if one does.
    pub fn staged_on(&self, dir: &Path, branch: &str) -> Option<&PatchRow> {
        let dir = dir.canonicalize().ok()?;
        self.rows.iter().find(|r| {
            send::branch_name(&r.id) == branch
                && self
                    .tracked(r)
                    .and_then(|t| send::clone_dir(&t.upstream).ok())
                    .and_then(|d| d.canonicalize().ok())
                    == Some(dir.clone())
        })
    }

    fn known(&self, id: &str) -> Result<()> {
        if !self.rows.iter().any(|r| r.id == id) {
            bail!("no patch {id}; `deck patches` lists them");
        }
        Ok(())
    }

    pub fn record(&mut self, id: &str, state: State, url: String, note: String) -> Result<()> {
        self.known(id)?;
        self.tracking.entry(id).events.push(Event {
            at: now(),
            state,
            url,
            note,
        });
        self.tracking.save(&self.track_path)?;
        self.rebuild_rows();
        Ok(())
    }

    pub fn set_upstream(&mut self, id: &str, url: String) -> Result<()> {
        self.known(id)?;
        self.tracking.entry(id).upstream = url;
        self.tracking.save(&self.track_path)?;
        self.rebuild_rows();
        Ok(())
    }
}
