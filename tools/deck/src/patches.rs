//! Local patches kept in the host's directory: user patches (portage/patches, deployed to
//! /etc/portage/patches) and patches the local overlay's ebuilds carry in files/.

use crate::vdb::{Pkg, dirs_sorted, split_pf};
use std::fs;
use std::path::{Path, PathBuf};

const SYSTEM_PATCHES: &str = "/etc/portage/patches";
const SYSTEM_OVERLAY: &str = "/var/db/repos/local";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    User,
    Overlay,
}

pub struct PatchFile {
    /// `category/package/file`: stays the same when the version directory is renamed on a bump.
    pub id: String,
    /// The path inside the host's directory.
    pub rel: String,
    pub kind: Kind,
    /// The `Subject:` of a git format-patch, without its `[PATCH]` tag.
    pub subject: String,
    /// Whether the system holds the same file, i.e. it was deployed.
    pub deployed: bool,
    /// The installed package it applies to, as an index into the package list.
    pub applies: Option<usize>,
}

fn is_patch(p: &Path) -> bool {
    p.is_file() && matches!(p.extension().and_then(|e| e.to_str()), Some("patch" | "diff"))
}

fn files_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for p in files_sorted(dir) {
        if p.is_dir() {
            walk(&p, out);
        } else if is_patch(&p) {
            out.push(p);
        }
    }
}

fn subject(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    for line in text.lines().take(80) {
        if let Some(s) = line.strip_prefix("Subject: ") {
            let s = s.trim();
            return match s.strip_prefix("[PATCH").and_then(|r| r.split_once(']')) {
                Some((_, rest)) => rest.trim().to_string(),
                None => s.to_string(),
            };
        }
    }
    String::new()
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((fs::read(a), fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

pub fn scan(host_dir: &Path, pkgs: &[Pkg]) -> Vec<PatchFile> {
    let mut out = Vec::new();

    // User patches: <cat>/<PN|P|PF>[:slot]/*.patch, as eapply_user reads them.
    let root = host_dir.join("portage/patches");
    for (cat, cat_dir) in dirs_sorted(&root) {
        for (name, dir) in dirs_sorted(&cat_dir) {
            let (atom, slot) = match name.split_once(':') {
                Some((a, s)) => (a, Some(s)),
                None => (name.as_str(), None),
            };
            let applies = pkgs.iter().position(|p| {
                p.cat == cat
                    && (p.pn == atom || p.p() == atom || p.pf() == atom)
                    && slot.is_none_or(|s| p.slot.split('/').next() == Some(s))
            });
            let pn = match applies {
                Some(i) => pkgs[i].pn.clone(),
                None => split_pf(atom).map_or(atom, |(pn, _)| pn).to_string(),
            };
            for file in files_sorted(&dir).into_iter().filter(|p| is_patch(p)) {
                let fname = file.file_name().unwrap().to_string_lossy().into_owned();
                let inside = file.strip_prefix(&root).unwrap();
                out.push(PatchFile {
                    id: format!("{cat}/{pn}/{fname}"),
                    rel: format!("portage/patches/{}", inside.display()),
                    kind: Kind::User,
                    subject: subject(&file),
                    deployed: same_file(&file, &Path::new(SYSTEM_PATCHES).join(inside)),
                    applies,
                });
            }
        }
    }

    // Patches carried by the local overlay's ebuilds, in <cat>/<pn>/files/.
    let root = host_dir.join("overlay");
    for (cat, cat_dir) in dirs_sorted(&root) {
        for (pn, dir) in dirs_sorted(&cat_dir) {
            let files_dir = dir.join("files");
            let mut found = Vec::new();
            walk(&files_dir, &mut found);
            let applies = pkgs
                .iter()
                .position(|p| p.cat == cat && p.pn == pn && p.repo == "local");
            for file in found {
                let inside = file.strip_prefix(&root).unwrap();
                let under_files = file.strip_prefix(&files_dir).unwrap();
                out.push(PatchFile {
                    id: format!("{cat}/{pn}/{}", under_files.display()),
                    rel: format!("overlay/{}", inside.display()),
                    kind: Kind::Overlay,
                    subject: subject(&file),
                    deployed: same_file(&file, &Path::new(SYSTEM_OVERLAY).join(inside)),
                    applies,
                });
            }
        }
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}
