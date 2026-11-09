//! A local patch in omnidiff: the files it changes, before and after, side by side.
//!
//! The source comes from the review cache (`review::prepare`: Portage unpacks and prepares the
//! installed version in the jail, once, and the tree is kept). That tree is what the build sees,
//! so our patches are already in it, unless its prepare failed and it was only unpacked (the
//! `<version>.unpatched` marker). From it, the files the patches touch are copied out, never
//! read in place, and our patch is taken back out (`patch -R`) to give the before side; the
//! patches after it in the same directory are taken out first, as they were applied after it
//! (eapply_user: in name order). On an unpatched tree the patches before it go in instead.
//!
//! Before is committed in a throwaway git repository and after left unstaged, and omnidiff opens
//! on its review picker there: every file the patch changes, one at a time.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::review;

/// The files a patch changes, relative to the source tree (as `patch -p1` reads it), in order.
pub fn touched(patch: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut old: Option<&str> = None;
    for line in patch.lines() {
        if let Some(p) = line.strip_prefix("--- ") {
            old = Some(p);
        } else if let Some(new) = line.strip_prefix("+++ ")
            && let Some(o) = old.take()
        {
            let side = if path_of(new) == "/dev/null" { o } else { new };
            let p = path_of(side);
            let p = p.split_once('/').map_or(p, |(_, rest)| rest);
            if !p.is_empty() && !out.iter().any(|x| x == p) {
                out.push(p.to_owned());
            }
        }
    }
    out
}

/// A `---`/`+++` line's path, without a trailing tab and timestamp.
fn path_of(side: &str) -> &str {
    side.split('\t').next().unwrap_or(side).trim()
}

/// The patches in DIR, in the order Portage applies them.
fn patches_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "patch" || x == "diff"))
        .collect();
    all.sort();
    Ok(all)
}

/// Copies REL from TREE to TO, when it is a plain file there: not through a symlink, which the
/// unpacked source could have made to lead out of the tree. A file the patch adds isn't there.
fn copy_out(tree: &Path, rel: &str, to: &Path) -> Result<()> {
    let from = tree.join(rel);
    let Ok(meta) = std::fs::symlink_metadata(&from) else {
        return Ok(());
    };
    if !meta.is_file() {
        bail!("{rel} is not a plain file in {}", tree.display());
    }
    let parent = from.parent().context("a file without a directory")?;
    if parent.canonicalize()?
        != tree
            .canonicalize()?
            .join(Path::new(rel).parent().unwrap_or(Path::new("")))
    {
        bail!("{rel} leads out of {}", tree.display());
    }
    let dest = to.join(rel);
    std::fs::create_dir_all(dest.parent().unwrap())?;
    std::fs::copy(&from, &dest).with_context(|| format!("copying {rel}"))?;
    Ok(())
}

fn apply(dir: &Path, patch: &Path, reverse: bool) -> Result<()> {
    let mut c = Command::new("patch");
    c.args(["-p1", "-s", "--no-backup-if-mismatch", "-f", "-d"])
        .arg(dir);
    if reverse {
        c.arg("-R");
    }
    let out = c.arg("-i").arg(patch).output().context("running patch")?;
    if !out.status.success() {
        bail!(
            "{} does not {} the installed source:\n{}",
            patch.file_name().unwrap_or_default().to_string_lossy(),
            if reverse { "come back out of" } else { "apply to" },
            String::from_utf8_lossy(&out.stdout).trim()
        );
    }
    Ok(())
}

/// Copies the files RELS from FROM to TO (TO emptied first); a file missing in FROM stays out.
fn snapshot(from: &Path, rels: &[String], to: &Path) -> Result<()> {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to)?;
    for rel in rels {
        let src = from.join(rel);
        if src.is_file() {
            let dest = to.join(rel);
            std::fs::create_dir_all(dest.parent().unwrap())?;
            std::fs::copy(&src, &dest)?;
        }
    }
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<()> {
    let st = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=deck",
            "-c",
            "user.email=deck@localhost",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdout(std::process::Stdio::null())
        .status()
        .context("running git")?;
    if !st.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

/// Builds the before/after repository for the patch at PATCH, of the installed CP-VERSION, and
/// returns it.
pub fn prepare(repo: &Path, patch: &Path, cp: &str, version: &str) -> Result<PathBuf> {
    let dir = patch.parent().context("patch without a directory")?;
    let all = patches_in(dir)?;
    let n = all.iter().position(|p| p == patch).with_context(|| {
        format!(
            "{} is not among the patches in {}",
            patch.display(),
            dir.display()
        )
    })?;
    let subject = review::Subject::Package {
        watched: None,
        cp: cp.to_owned(),
    };
    let versions = review::versions(repo, std::slice::from_ref(&subject));
    let v = versions
        .get(cp)
        .with_context(|| format!("{cp}: Portage doesn't know it"))?;
    eprintln!(
        "deck-patches: the source of {cp}-{version} (unpacked in the jail the first time, then cached)"
    );
    let tree = review::prepare(repo, &subject, version, v, &mut std::io::stderr())?;
    let unpatched = tree.with_file_name(format!("{version}.unpatched")).exists();

    let base = review::cache_dir().join("view").join(subject.slug());
    let _ = std::fs::remove_dir_all(&base);
    let scratch = base.join("scratch");
    std::fs::create_dir_all(&scratch)?;
    let involved = if unpatched { &all[..=n] } else { &all[n..] };
    let mut rels: Vec<String> = Vec::new();
    for p in involved {
        for r in touched(&std::fs::read_to_string(p)?) {
            if !rels.contains(&r) {
                rels.push(r);
            }
        }
    }
    for r in &rels {
        copy_out(&tree, r, &scratch)?;
    }
    let own = touched(&std::fs::read_to_string(patch)?);
    let (before, after) = (base.join("before"), base.join("after"));
    if unpatched {
        for p in &all[..n] {
            apply(&scratch, p, false)?;
        }
        snapshot(&scratch, &own, &before)?;
        apply(&scratch, patch, false)?;
        snapshot(&scratch, &own, &after)?;
    } else {
        for p in all[n + 1..].iter().rev() {
            apply(&scratch, p, true)?;
        }
        snapshot(&scratch, &own, &after)?;
        apply(&scratch, patch, true)?;
        snapshot(&scratch, &own, &before)?;
    }

    let view = base.join(format!("{cp}-{version}").replace('/', "_"));
    std::fs::create_dir_all(&view)?;
    git(&view, &["init", "-q"])?;
    for r in &own {
        if before.join(r).is_file() {
            let d = view.join(r);
            std::fs::create_dir_all(d.parent().unwrap())?;
            std::fs::copy(before.join(r), &d)?;
        }
    }
    git(&view, &["add", "-A"])?;
    git(
        &view,
        &["commit", "-q", "--allow-empty", "-m", "before the patch"],
    )?;
    for r in &own {
        let d = view.join(r);
        if after.join(r).is_file() {
            std::fs::create_dir_all(d.parent().unwrap())?;
            std::fs::copy(after.join(r), &d)?;
        } else {
            let _ = std::fs::remove_file(&d);
        }
    }
    // Added files show up too: intent-to-add puts them in the diff against the index.
    git(&view, &["add", "-N", "."])?;
    Ok(view)
}

/// Shows the patch in omnidiff; when that can't be (no omnidiff, a source that won't unpack, a
/// patch that doesn't come out cleanly), says why and shows the patch with less instead.
pub fn show(repo: &Path, patch: &Path, target: Option<(String, String)>) -> Result<()> {
    let tried = match (&target, which("omnidiff")) {
        (_, false) => Err(anyhow::anyhow!("omnidiff is not installed")),
        (None, _) => Err(anyhow::anyhow!("the package it applies to isn't installed")),
        (Some((cp, version)), true) => prepare(repo, patch, cp, version),
    };
    match tried {
        Ok(view) => {
            let st = Command::new("omnidiff")
                .arg("--review")
                .current_dir(&view)
                .status()?;
            if !st.success() {
                bail!("omnidiff failed");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("deck-patches: not in omnidiff: {e:#}\n(Enter for the patch itself)");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            Command::new("less").arg("-R").arg(patch).status()?;
            Ok(())
        }
    }
}

/// Opens `deck-patches show ID` in a terminal of its own, for the TUI.
pub fn open(host_dir: &Path, id: &str) -> Result<()> {
    static OPENED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = OPENED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let exe = std::env::current_exe().context("finding deck-patches")?;
    let mut child = Command::new("systemd-run")
        .args(["--user", "--scope", "--slice=app.slice", "--collect", "--quiet"])
        .arg(format!(
            "--unit=app-foot-deck-show-{}-{n}.scope",
            std::process::id()
        ))
        .args(["--", "foot", "--"])
        .arg(exe)
        .arg("--host-dir")
        .arg(host_dir)
        .args(["show", id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("starting foot")?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty directory of the test's own, as the other tests here make them.
    fn tempdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deck-patchview-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn touched_reads_changed_added_and_removed_files() {
        let patch = "intro\n\ndiff --git a/src/x.rs b/src/x.rs\n--- a/src/x.rs\n+++ b/src/x.rs\n@@ -1 +1 @@\n-a\n+b\n\
                     --- /dev/null\n+++ b/new.txt\t2026-01-01\n@@ -0,0 +1 @@\n+n\n\
                     --- a/gone.c\n+++ /dev/null\n@@ -1 +0,0 @@\n-g\n\
                     --- a/src/x.rs\n+++ b/src/x.rs\n@@ -5 +5 @@\n-c\n+d\n";
        assert_eq!(touched(patch), ["src/x.rs", "new.txt", "gone.c"]);
    }

    #[test]
    fn before_and_after_come_from_taking_the_patch_back_out() {
        let dir = tempdir("apply");
        let tree = dir.join("tree");
        std::fs::create_dir_all(tree.join("src")).unwrap();
        // The tree as the build sees it: the patch already in.
        std::fs::write(tree.join("src/x.rs"), "one\nTWO\nthree\n").unwrap();
        let patch = dir.join("0001.patch");
        std::fs::write(
            &patch,
            "--- a/src/x.rs\n+++ b/src/x.rs\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n",
        )
        .unwrap();
        let scratch = dir.join("scratch");
        copy_out(&tree, "src/x.rs", &scratch).unwrap();
        apply(&scratch, &patch, true).unwrap();
        assert_eq!(
            std::fs::read_to_string(scratch.join("src/x.rs")).unwrap(),
            "one\ntwo\nthree\n"
        );
        // The tree itself is never touched.
        assert_eq!(
            std::fs::read_to_string(tree.join("src/x.rs")).unwrap(),
            "one\nTWO\nthree\n"
        );
    }

    #[test]
    fn a_symlink_in_the_source_is_not_followed() {
        let dir = tempdir("symlink");
        let tree = dir.join("tree");
        std::fs::create_dir_all(&tree).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", tree.join("x")).unwrap();
        assert!(copy_out(&tree, "x", &dir.join("out")).is_err());
    }
}
