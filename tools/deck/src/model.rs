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

/// ~/.config/deck/host-dir names the directory of a host kept outside this repository (a path,
/// relative to the home or absolute); it holds even where the environment is cleared, as for the
/// emerge wrapper's gate. Without it, ~/src/agentic-gentoo/hosts/<hostname>.
pub fn default_host_dir() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    if let Ok(dir) = fs::read_to_string(home.join(".config/deck/host-dir")) {
        return Ok(home.join(dir.trim()));
    }
    let host = fs::read_to_string("/etc/hostname").context("reading /etc/hostname")?;
    Ok(home.join("src/agentic-gentoo/hosts").join(host.trim()))
}

impl Model {
    pub fn load(host_dir: PathBuf) -> Result<Model> {
        if !host_dir.is_dir() {
            bail!(
                "{} is not a directory; pass --host-dir, set DECK_HOST_DIR or name it in ~/.config/deck/host-dir",
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

    /// The upstream project the patch is meant for, or an error saying how to set it.
    pub fn upstream_of(&self, id: &str) -> Result<&str> {
        self.known(id)?;
        self.rows
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| self.tracked(r))
            .map(|t| t.upstream.as_str())
            .filter(|u| !u.is_empty())
            .with_context(|| format!("{id} has no upstream project: U, or deck patches upstream {id} <URL>"))
    }

    /// Stages the patch's file on a branch of its upstream project's clone (`send::stage`).
    pub fn stage(&self, id: &str) -> Result<send::Staged> {
        let upstream = self.upstream_of(id)?;
        let row = self.rows.iter().find(|r| r.id == id).context("no such patch")?;
        let file = row
            .file
            .map(|i| &self.files[i])
            .with_context(|| format!("{id}: its file is gone"))?;
        send::stage(upstream, id, &self.host_dir.join(&file.rel))
    }

    /// How far the patch has come on its way upstream (`send::progress`).
    pub fn progress(&self, id: &str, look_up_pr: bool) -> Result<send::Progress> {
        send::progress(self.upstream_of(id)?, id, look_up_pr)
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
            bail!("no patch {id}; `deck patches patches` lists them");
        }
        Ok(())
    }

    /// Both change upstream.toml as it is now, not as it was loaded: what was added to it
    /// meanwhile (another `deck patches`, a hand edit) is kept, and shows from here on.
    pub fn record(&mut self, id: &str, state: State, url: String, note: String) -> Result<()> {
        self.known(id)?;
        let event = Event {
            at: now(),
            state,
            url,
            note,
        };
        self.tracking = Tracking::add_event(&self.track_path, id, &event)?;
        self.rebuild_rows();
        Ok(())
    }

    pub fn set_upstream(&mut self, id: &str, url: String) -> Result<()> {
        self.known(id)?;
        self.tracking = Tracking::set_upstream(&self.track_path, id, &url)?;
        self.rebuild_rows();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every patch ever tracked stays listed, whatever its state, after its file is deleted.
    #[test]
    fn a_patch_without_its_file_is_still_listed() {
        let tracked = |id: &str, state| Tracked {
            id: id.into(),
            upstream: String::new(),
            events: vec![Event {
                at: now(),
                state,
                url: String::new(),
                note: String::new(),
            }],
        };
        let mut m = Model {
            host_dir: PathBuf::new(),
            track_path: PathBuf::new(),
            pkgs: vec![],
            files: vec![],
            tracking: Tracking {
                patches: vec![
                    tracked("cat/pkg/0001-a.patch", State::Released),
                    tracked("cat/pkg/0002-b.patch", State::Dropped),
                ],
            },
            rows: vec![],
            patch_counts: vec![],
        };
        m.rebuild_rows();
        let rows: Vec<_> = m
            .rows
            .iter()
            .map(|r| (r.id.as_str(), m.state_label(r), m.applies_label(r)))
            .collect();
        assert_eq!(
            rows,
            [
                ("cat/pkg/0001-a.patch", "released", "(file gone)".to_string()),
                ("cat/pkg/0002-b.patch", "dropped", "(file gone)".to_string()),
            ]
        );
    }
}
