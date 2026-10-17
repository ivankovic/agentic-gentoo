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

/// Runs a command in `dir` on the user's terminal, for the steps that may ask something.
fn interactive(dir: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("running {cmd}"))?;
    if !status.success() {
        bail!("{cmd} {} failed", args.join(" "));
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
    let unit = format!("--unit=app-foot-deck-{}.scope", std::process::id());
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
pub fn fork(dir: &Path) -> Result<()> {
    if let Ok(repo) = remote_repo(dir, "fork") {
        println!("deck: the fork is there already: remote fork, {repo}");
        return Ok(());
    }
    interactive(dir, "gh", &["repo", "fork", "--remote", "--remote-name", "fork"])
}

/// Pushes the current branch to your fork, once it has a commit of its own.
pub fn push(dir: &Path) -> Result<()> {
    let branch = current_branch(dir)?;
    let base = default_base(dir)?;
    if git(dir, &["rev-list", "--count", &format!("{base}..HEAD")])? == "0" {
        bail!("{branch} has no commit on top of {base} yet: git commit first");
    }
    if remote_repo(dir, "fork").is_err() {
        bail!("no remote fork yet: run deck fork first");
    }
    if !git(dir, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        println!("deck: note: uncommitted changes stay here, only commits are pushed");
    }
    interactive(dir, "git", &["push", "-u", "fork", &format!("HEAD:{branch}")])
}

/// What `pr` found: the pull request's URL once there is one.
pub struct Pr {
    pub branch: String,
    pub url: Option<String>,
}

/// Opens the browser to write the pull request from your fork's branch to upstream, with
/// upstream's template; once it exists, returns its URL instead.
pub fn pr(dir: &Path) -> Result<Pr> {
    let branch = current_branch(dir)?;
    let repo = remote_repo(dir, "origin")?;
    let fork = remote_repo(dir, "fork").context("no remote fork yet: run deck fork first")?;
    let owner = fork.split('/').next().unwrap_or_default();
    let out = Command::new("gh")
        .args(["pr", "list", "--repo", &repo, "--state", "all", "--head", &branch])
        .args(["--json", "url,headRepositoryOwner", "--jq"])
        .arg(format!(
            ".[] | select(.headRepositoryOwner.login == \"{owner}\") | .url"
        ))
        .stdin(Stdio::null())
        .output()
        .context("running gh pr list")?;
    let found = String::from_utf8_lossy(&out.stdout);
    if let Some(url) = found.lines().next().filter(|u| !u.is_empty()) {
        return Ok(Pr {
            branch,
            url: Some(url.to_string()),
        });
    }
    let base = default_base(dir)?;
    let base = base.trim_start_matches("origin/");
    interactive(
        dir,
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
