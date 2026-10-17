//! Taking a local patch upstream, one step at a time: `stage` puts it on a branch of a clone in
//! ~/3p, staged and uncommitted, since the commit message and the PR text are the user's to
//! write; `fork`, `push` and `pr` each do one more step, only when run. Nothing reaches other
//! people until `push`.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `~/3p/<host>/<owner>/<repo>` for an upstream URL such as `https://github.com/owner/repo`.
pub fn clone_dir(upstream: &str) -> Result<PathBuf> {
    let rest = upstream
        .split_once("://")
        .map_or(upstream, |(_, r)| r)
        .trim_end_matches('/')
        .trim_end_matches(".git");
    if rest.split('/').filter(|s| !s.is_empty()).count() < 3 {
        bail!("{upstream}: expected https://<host>/<owner>/<repo>");
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join("3p").join(rest))
}

/// The branch a patch goes on: its file name without the number and the extension, so
/// `0002-ask-before-a-new-game.patch` goes on `ask-before-a-new-game`.
pub fn branch_name(id: &str) -> String {
    let file = id.rsplit('/').next().unwrap_or(id);
    let stem = file.trim_end_matches(".patch").trim_end_matches(".diff");
    match stem.split_once('-') {
        Some((n, rest)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => rest.into(),
        _ => stem.into(),
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("running git")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Runs a command in `dir`: on the user's terminal with `tty`, so it can ask something;
/// otherwise with nothing to read and its output kept, as the TUI runs it.
fn run(dir: &Path, tty: bool, cmd: &str, args: &[&str]) -> Result<()> {
    let mut c = Command::new(cmd);
    c.args(args).current_dir(dir);
    let (ok, err) = if tty {
        let st = c.status().with_context(|| format!("running {cmd}"))?;
        (st.success(), String::new())
    } else {
        let out = c
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running {cmd}"))?;
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
        (out.status.success(), format!(": {}", last.trim()))
    };
    if !ok {
        bail!("{cmd} {} failed{err}", args.join(" "));
    }
    Ok(())
}

/// `origin/main`, or whatever upstream's default branch is.
fn default_base(dir: &Path) -> Result<String> {
    let head = ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"];
    if let Ok(r) = git(dir, &head) {
        return Ok(r);
    }
    git(dir, &["remote", "set-head", "origin", "--auto"])?;
    git(dir, &head)
}

pub struct Staged {
    pub dir: PathBuf,
    pub branch: String,
    /// False when the branch was there already and was left as it was.
    pub fresh: bool,
}

/// Clones upstream if there is no clone yet, and on a new branch from its default branch stages
/// the patch with `git apply --3way --index`, without committing. A branch of that name that
/// exists already is left alone, so running it again only finds the clone.
pub fn stage(upstream: &str, id: &str, patch: &Path) -> Result<Staged> {
    let dir = clone_dir(upstream)?;
    if !dir.join(".git").exists() {
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)?;
        }
        let ok = Command::new("git")
            .args(["clone", "--quiet", upstream])
            .arg(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("running git clone")?
            .success();
        if !ok {
            bail!("git clone {upstream} failed");
        }
    }
    let branch = branch_name(id);
    let exists = [
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ];
    if git(&dir, &exists).is_ok() {
        return Ok(Staged {
            dir,
            branch,
            fresh: false,
        });
    }
    if !git(&dir, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        bail!(
            "{} has uncommitted changes; commit or stash them first",
            dir.display()
        );
    }
    git(&dir, &["fetch", "--quiet", "origin"])?;
    let base = default_base(&dir)?;
    git(&dir, &["switch", "--quiet", "--no-track", "-c", &branch, &base])?;
    let patch = patch.to_str().context("patch path is not UTF-8")?;
    if let Err(e) = git(&dir, &["apply", "--3way", "--index", patch]) {
        bail!(
            "on branch {branch} in {}, the patch doesn't apply cleanly to {base}; resolve it there ({e:#})",
            dir.display()
        );
    }
    Ok(Staged {
        dir,
        branch,
        fresh: true,
    })
}

/// Opens a terminal in `dir` in its own scope, as Mod+T does, without waiting for it.
pub fn terminal(dir: &Path) -> Result<()> {
    // A unit name may be used once at a time: the process id and a count keep them apart.
    static OPENED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = OPENED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let unit = format!("--unit=app-foot-deck-{}-{n}.scope", std::process::id());
    let mut child = Command::new("systemd-run")
        .args(["--user", "--scope", "--slice=app.slice", "--collect", "--quiet"])
        .arg(&unit)
        .args(["--", "foot", "-D"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting foot")?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// `owner/repo` from a GitHub remote URL, https or ssh.
fn github_repo(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    Some(rest.trim_end_matches('/').trim_end_matches(".git").to_string())
}

fn remote_repo(dir: &Path, remote: &str) -> Result<String> {
    let url = git(dir, &["remote", "get-url", remote])?;
    github_repo(&url).with_context(|| format!("remote {remote} ({url}) is not on GitHub"))
}

fn current_branch(dir: &Path) -> Result<String> {
    let b = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if b == "HEAD" {
        bail!("{}: not on a branch", dir.display());
    }
    Ok(b)
}

/// Makes your fork on GitHub and adds it as the remote `fork`; `origin` stays upstream.
/// Returns the fork, `owner/repo`.
pub fn fork(dir: &Path, tty: bool) -> Result<String> {
    if let Ok(repo) = remote_repo(dir, "fork") {
        return Ok(repo);
    }
    run(
        dir,
        tty,
        "gh",
        &["repo", "fork", "--remote", "--remote-name", "fork"],
    )?;
    remote_repo(dir, "fork")
}

/// Pushes the current branch to your fork, once it has a commit of its own.
pub fn push(dir: &Path, tty: bool) -> Result<()> {
    let branch = current_branch(dir)?;
    let base = default_base(dir)?;
    if git(dir, &["rev-list", "--count", &format!("{base}..HEAD")])? == "0" {
        bail!("{branch} has no commit on top of {base} yet: git commit first");
    }
    if remote_repo(dir, "fork").is_err() {
        bail!("no remote fork yet: run deck fork first");
    }
    if tty && !git(dir, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        println!("deck: note: uncommitted changes stay here, only commits are pushed");
    }
    run(
        dir,
        tty,
        "git",
        &["push", "-u", "fork", &format!("HEAD:{branch}")],
    )
}

/// What `pr` found: the pull request's URL once there is one.
pub struct Pr {
    pub branch: String,
    pub url: Option<String>,
}

/// Opens the browser to write the pull request from your fork's branch to upstream, with
/// upstream's template; once it exists, returns its URL instead.
pub fn pr(dir: &Path, tty: bool) -> Result<Pr> {
    let branch = current_branch(dir)?;
    let repo = remote_repo(dir, "origin")?;
    let fork = remote_repo(dir, "fork").context("no remote fork yet: run deck fork first")?;
    let owner = fork.split('/').next().unwrap_or_default();
    if let Some(url) = find_pr(&repo, owner, &branch)? {
        return Ok(Pr {
            branch,
            url: Some(url),
        });
    }
    let base = default_base(dir)?;
    let base = base.trim_start_matches("origin/");
    run(
        dir,
        tty,
        "gh",
        &[
            "pr",
            "create",
            "--web",
            "--repo",
            &repo,
            "--base",
            base,
            "--head",
            &format!("{owner}:{branch}"),
        ],
    )?;
    Ok(Pr { branch, url: None })
}

/// The pull request, open or not, from `owner`'s `branch` to upstream `repo`.
fn find_pr(repo: &str, owner: &str, branch: &str) -> Result<Option<String>> {
    let out = Command::new("gh")
        .args(["pr", "list", "--repo", repo, "--state", "all", "--head", branch])
        .args(["--json", "url,headRepositoryOwner", "--jq"])
        .arg(format!(
            ".[] | select(.headRepositoryOwner.login == \"{owner}\") | .url"
        ))
        .stdin(Stdio::null())
        .output()
        .context("running gh pr list")?;
    if !out.status.success() {
        bail!(
            "gh pr list failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let found = String::from_utf8_lossy(&out.stdout);
    Ok(found.lines().next().filter(|u| !u.is_empty()).map(str::to_string))
}

/// How far a patch has come on its way upstream, from what is on disk, plus the pull request
/// when asked to look it up (that asks GitHub).
pub struct Progress {
    pub dir: PathBuf,
    pub branch: String,
    pub cloned: bool,
    pub branched: bool,
    /// Changes on the branch not committed yet, staged or not.
    pub uncommitted: bool,
    /// Commits on the branch on top of upstream's default branch.
    pub commits: usize,
    /// Your fork, `owner/repo`, once it is the remote `fork`.
    pub fork: Option<String>,
    /// The fork's branch is at the branch's last commit.
    pub pushed: bool,
    pub pr: Option<String>,
}

/// The next thing to do, after `Progress`.
#[derive(Debug, PartialEq)]
pub enum Step {
    Stage,
    /// The branch is checked out elsewhere, or the clone is on another branch.
    SwitchBranch,
    Commit,
    Fork,
    Push,
    Pr,
    /// There is a pull request: nothing left here but to follow it.
    Follow(String),
}

pub fn progress(upstream: &str, id: &str, look_up_pr: bool) -> Result<Progress> {
    let dir = clone_dir(upstream)?;
    let branch = branch_name(id);
    let mut p = Progress {
        cloned: dir.join(".git").exists(),
        dir,
        branch,
        branched: false,
        uncommitted: false,
        commits: 0,
        fork: None,
        pushed: false,
        pr: None,
    };
    if !p.cloned {
        return Ok(p);
    }
    let d = p.dir.clone();
    let b = p.branch.clone();
    p.branched = git(
        &d,
        &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}")],
    )
    .is_ok();
    if !p.branched {
        return Ok(p);
    }
    if let Ok(base) = default_base(&d) {
        p.commits = git(&d, &["rev-list", "--count", &format!("{base}..{b}")])?
            .parse()
            .unwrap_or(0);
    }
    if current_branch(&d).ok().as_deref() == Some(&b) {
        p.uncommitted = !git(&d, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
    }
    p.fork = remote_repo(&d, "fork").ok();
    let tip = git(&d, &["rev-parse", &format!("refs/heads/{b}")])?;
    p.pushed = git(
        &d,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/fork/{b}"),
        ],
    )
    .is_ok_and(|r| r == tip);
    if look_up_pr
        && let Some(fork) = &p.fork
        && let Ok(repo) = remote_repo(&d, "origin")
    {
        let owner = fork.split('/').next().unwrap_or_default();
        p.pr = find_pr(&repo, owner, &b)?;
    }
    Ok(p)
}

impl Progress {
    /// After a pull request exists, new commits still go through Push: that is how changes
    /// upstream asked for reach it.
    pub fn next(&self) -> Step {
        if !self.branched {
            return match &self.pr {
                Some(url) => Step::Follow(url.clone()),
                None => Step::Stage,
            };
        }
        if current_branch(&self.dir).ok().as_deref() != Some(self.branch.as_str()) {
            return Step::SwitchBranch;
        }
        if self.uncommitted || self.commits == 0 {
            return Step::Commit;
        }
        if self.fork.is_none() {
            return Step::Fork;
        }
        if !self.pushed {
            return Step::Push;
        }
        match &self.pr {
            Some(url) => Step::Follow(url.clone()),
            None => Step::Pr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            branch_name("games-board/chess-tui/0002-ask-before-a-new-game.patch"),
            "ask-before-a-new-game"
        );
        assert_eq!(branch_name("a/b/fix-build.diff"), "fix-build");
        assert_eq!(
            github_repo("git@github.com:thomas-mauran/chess-tui.git").as_deref(),
            Some("thomas-mauran/chess-tui")
        );
        assert_eq!(
            github_repo("https://github.com/ivankovic/chess-tui").as_deref(),
            Some("ivankovic/chess-tui")
        );
        let d = clone_dir("https://github.com/thomas-mauran/chess-tui/").unwrap();
        assert!(
            d.ends_with("3p/github.com/thomas-mauran/chess-tui"),
            "{}",
            d.display()
        );
        assert!(clone_dir("https://github.com/x").is_err());
    }
}
