//! Which package owns a file: every regular file in /var/db/pkg/*/*/CONTENTS, keyed by its path
//! with the directories resolved. /bin, /sbin and /lib are links into /usr here, so a package that
//! records /bin/bash owns what the kernel reports as /usr/bin/bash.

use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::time::SystemTime;

const VDB: &str = "/var/db/pkg";
/// Portage bumps it on every merge.
const COUNTER: &str = "/var/cache/edb/counter";

#[derive(Default)]
pub struct PkgMap {
    /// Path hash → index into `cps`. Hashes keep 375k paths in a few MiB; a collision would
    /// need two installed files with the same 64-bit hash.
    by_path: HashMap<u64, u32>,
    /// `category/package`, without the version: usage carries over an upgrade.
    pub cps: Vec<String>,
    /// The package database's `stamp()` when the map was built.
    pub stamp: Option<SystemTime>,
    /// Each directory named in CONTENTS, resolved: kept for the next build, which then only
    /// resolves the directories new packages bring.
    canon: HashMap<String, String>,
}

fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// When Portage last merged or removed a package.
pub fn stamp() -> Option<SystemTime> {
    fs::metadata(COUNTER).and_then(|m| m.modified()).ok()
}

/// `1.1.0-r1` → `1.1.0`: a version without its revision.
pub fn strip_rev(v: &str) -> &str {
    match v.rfind("-r") {
        Some(i) if i + 2 < v.len() && v[i + 2..].bytes().all(|b| b.is_ascii_digit()) => &v[..i],
        _ => v,
    }
}

/// Splits `PN-PV[-rN]` into PN and the version with its revision. Package names may hold a dash
/// followed by a digit (`font-adobe-100dpi`), versions never hold a dash other than the revision's,
/// so the version starts after the last dash followed by a digit.
pub fn split_pf(pf: &str) -> Option<(&str, &str)> {
    let base = strip_rev(pf);
    let i = base
        .match_indices('-')
        .map(|(i, _)| i)
        .rfind(|&i| base[i + 1..].starts_with(|c: char| c.is_ascii_digit()))?;
    Some((&pf[..i], &pf[i + 1..]))
}

impl PkgMap {
    pub fn build() -> PkgMap {
        PkgMap::rebuild(HashMap::new())
    }

    /// Built again after a merge, from the directories this one resolved.
    pub fn rebuilt(self) -> PkgMap {
        PkgMap::rebuild(self.canon)
    }

    fn rebuild(mut canon: HashMap<String, String>) -> PkgMap {
        let stamp = stamp();
        let mut by_path = HashMap::with_capacity(400_000);
        let mut cps: Vec<String> = Vec::new();
        let mut entries: Vec<_> = fs::read_dir(VDB)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for cat_dir in entries {
            let cat = cat_dir.file_name().unwrap().to_string_lossy().into_owned();
            let mut pkgs: Vec<_> = fs::read_dir(&cat_dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            pkgs.sort();
            for dir in pkgs {
                let pf = dir.file_name().unwrap().to_string_lossy().into_owned();
                // Portage leaves `-MERGING-…` directories behind while it works.
                if pf.starts_with('-') {
                    continue;
                }
                let Ok(contents) = fs::read_to_string(dir.join("CONTENTS")) else {
                    continue;
                };
                let cp = format!("{cat}/{}", split_pf(&pf).map_or(pf.as_str(), |(pn, _)| pn));
                let ix = match cps.iter().position(|c| *c == cp) {
                    Some(i) => i,
                    None => {
                        cps.push(cp);
                        cps.len() - 1
                    }
                } as u32;
                for line in contents.lines() {
                    // `obj <path> <md5> <mtime>`; the path may hold spaces.
                    let Some(rest) = line.strip_prefix("obj ") else {
                        continue;
                    };
                    let Some(path) = rest.rsplitn(3, ' ').nth(2) else {
                        continue;
                    };
                    let Some((d, name)) = path.rsplit_once('/') else {
                        continue;
                    };
                    let real = canon.entry(d.to_string()).or_insert_with(|| {
                        fs::canonicalize(if d.is_empty() { "/" } else { d })
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_else(|_| d.to_string())
                    });
                    let full = if real == "/" {
                        format!("/{name}")
                    } else {
                        format!("{real}/{name}")
                    };
                    by_path.insert(hash(&full), ix);
                    by_path.insert(hash(path), ix);
                }
            }
        }
        PkgMap {
            by_path,
            cps,
            stamp,
            canon,
        }
    }

    pub fn lookup(&self, path: &str) -> Option<&str> {
        self.by_path
            .get(&hash(path))
            .map(|&i| self.cps[i as usize].as_str())
    }

    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }
}

/// Whether a program runs scripts, so the script names the package rather than the program.
pub fn is_interpreter(exe: &str) -> bool {
    let base = exe.rsplit('/').next().unwrap_or(exe);
    let base = base.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    matches!(
        base,
        "python" | "perl" | "ruby" | "bash" | "sh" | "dash" | "zsh" | "lua" | "luajit" | "node" | "tclsh"
    )
}

/// A name for a program no package owns.
pub fn unowned(path: &str) -> String {
    let file = Path::new(path)
        .file_name()
        .map_or(path.to_string(), |f| f.to_string_lossy().into_owned());
    // /usr/local and /etc: this machine's own scripts (deck-mail, deck-vitals).
    if path.starts_with("/usr/local/") || path.starts_with("/etc/") {
        return format!("local:{file}");
    }
    if let Some(rest) = path
        .strip_prefix("/home/")
        .and_then(|r| r.split_once('/'))
        .map(|(_, r)| r)
    {
        let parts: Vec<&str> = rest.split('/').collect();
        if let Some(i) = parts.iter().position(|p| *p == "target")
            && matches!(parts.get(i + 1), Some(&"debug" | &"release"))
        {
            // Test binaries carry cargo's hash: my_app-6f85dff91071ea47.
            let name = match file.rsplit_once('-') {
                Some((n, h)) if h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()) => n,
                _ => file.as_str(),
            };
            return format!("user:build:{name}");
        }
        return match parts.as_slice() {
            [".local", "share", app, ..] => format!("user:{app}"),
            [".local", "bin", ..] => format!("user:bin:{file}"),
            [".cargo", "bin", ..] => format!("user:cargo:{file}"),
            [".rustup", ..] => format!("user:rustup:{file}"),
            [".bun", ..] => "user:bun".to_string(),
            [dir, ..] => format!("user:~/{dir}"),
            [] => "user".to_string(),
        };
    }
    if path.starts_with("/tmp/") || path.starts_with("/var/tmp/") {
        return format!("tmp:{file}");
    }
    if path.starts_with("/memfd:") {
        return "memfd".to_string();
    }
    format!("unowned:{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_unowned_programs() {
        assert_eq!(
            unowned("/home/user/.local/share/claude/versions/2.1.300"),
            "user:claude"
        );
        assert_eq!(
            unowned("/home/user/src/app/target/debug/app"),
            "user:build:app"
        );
        assert_eq!(
            unowned("/home/user/src/x/target/debug/deps/my_app-6f85dff91071ea47"),
            "user:build:my_app"
        );
        assert_eq!(
            unowned("/home/user/.rustup/toolchains/x/bin/rust-analyzer"),
            "user:rustup:rust-analyzer"
        );
        assert_eq!(unowned("/usr/local/bin/deck-terminals"), "local:deck-terminals");
        assert_eq!(unowned("/etc/deck-vitals/vitals.py"), "local:vitals.py");
        assert_eq!(unowned("/opt/thing/bin/x"), "unowned:/opt/thing/bin/x");
    }

    #[test]
    fn splits_pf() {
        assert_eq!(split_pf("chess-tui-2.7.1"), Some(("chess-tui", "2.7.1")));
        assert_eq!(
            split_pf("kramdown-parser-gfm-1.1.0-r1"),
            Some(("kramdown-parser-gfm", "1.1.0-r1"))
        );
        assert_eq!(
            split_pf("font-adobe-100dpi-1.0.4"),
            Some(("font-adobe-100dpi", "1.0.4"))
        );
        assert_eq!(
            split_pf("intel-microcode-20260812_p20260813"),
            Some(("intel-microcode", "20260812_p20260813"))
        );
        assert_eq!(split_pf("acct-group"), None);
    }

    #[test]
    fn strips_revision_only() {
        assert_eq!(strip_rev("1.1.0-r1"), "1.1.0");
        assert_eq!(strip_rev("1.1.0"), "1.1.0");
        assert_eq!(strip_rev("0-r"), "0-r");
    }
}
