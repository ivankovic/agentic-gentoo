//! Installed packages, read from Portage's database in /var/db/pkg.

use anyhow::{Context, Result};
use clap::ValueEnum;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const VDB: &str = "/var/db/pkg";
const WORLD: &str = "/var/lib/portage/world";

/// Where a package's installed files came from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Origin {
    /// A binary package, fetched from the binhost.
    Binhost,
    /// Built here, and the build produced machine code.
    Compiled,
    /// Built here from upstream's own binaries: a -bin package.
    Prebuilt,
    /// Built here, with no machine code installed: scripts, data, fonts, firmware.
    Noarch,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Binhost => "binhost",
            Origin::Compiled => "compiled",
            Origin::Prebuilt => "prebuilt",
            Origin::Noarch => "noarch",
        }
    }

    pub fn explain(self) -> &'static str {
        match self {
            Origin::Binhost => "binary package from the binhost",
            Origin::Compiled => "compiled from source on this machine",
            Origin::Prebuilt => "installed on this machine from upstream's prebuilt binaries",
            Origin::Noarch => {
                "installed on this machine, with no machine code (scripts, data, fonts, firmware)"
            }
        }
    }
}

pub struct Pkg {
    pub cat: String,
    pub pn: String,
    /// Version with any revision, e.g. `1.1.0-r1`.
    pub pvr: String,
    pub slot: String,
    pub repo: String,
    pub origin: Origin,
    pub build_time: i64,
    pub size: u64,
    pub world: bool,
    /// The language ecosystem whose packages it vendors, when there is one: rust, go, python, ruby.
    pub eco: &'static str,
    pub description: String,
    pub homepage: String,
    pub use_flags: String,
}

impl Pkg {
    pub fn cp(&self) -> String {
        format!("{}/{}", self.cat, self.pn)
    }

    pub fn atom(&self) -> String {
        format!("{}/{}", self.cat, self.pf())
    }

    /// `PN-PV-rN`, as Portage's PF.
    pub fn pf(&self) -> String {
        format!("{}-{}", self.pn, self.pvr)
    }

    /// `PN-PV` without the revision, as Portage's P.
    pub fn p(&self) -> String {
        format!("{}-{}", self.pn, strip_rev(&self.pvr))
    }
}

fn strip_rev(v: &str) -> &str {
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

pub fn dirs_sorted(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    out.sort();
    out
}

fn field(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn eco(inherited: &str) -> &'static str {
    for e in inherited.split_whitespace() {
        match e {
            "cargo" => return "rust",
            "go-module" => return "go",
            "distutils-r1" => return "python",
            "ruby-fakegem" => return "ruby",
            _ => {}
        }
    }
    ""
}

/// Whether the package installed compiled code: ELF, PE (EFI), LLVM bitcode or SPIR-V. Firmware is
/// left out, since some blobs are ELF but none run on the CPU.
fn installs_machine_code(dir: &Path) -> bool {
    let contents = fs::read_to_string(dir.join("CONTENTS")).unwrap_or_default();
    for line in contents.lines() {
        // `obj <path> <md5> <mtime>`; the path may hold spaces.
        let Some(rest) = line.strip_prefix("obj ") else {
            continue;
        };
        let Some(path) = rest.rsplitn(3, ' ').nth(2) else {
            continue;
        };
        if path.contains("/firmware/") || path.starts_with("/usr/share/doc/") {
            continue;
        }
        let mut magic = [0u8; 4];
        if File::open(path)
            .and_then(|mut f| f.read_exact(&mut magic))
            .is_ok()
            && (magic == *b"\x7fELF"
                || magic[..2] == *b"MZ"
                || magic == *b"BC\xc0\xde"
                || magic == [0x03, 0x02, 0x23, 0x07])
        {
            return true;
        }
    }
    false
}

pub fn load() -> Result<Vec<Pkg>> {
    let world: HashSet<String> = fs::read_to_string(WORLD)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split(':').next())
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let vdb = Path::new(VDB);
    fs::metadata(vdb).with_context(|| format!("reading {VDB}"))?;

    let mut pkgs = Vec::new();
    for (cat, cat_dir) in dirs_sorted(vdb) {
        for (pf, dir) in dirs_sorted(&cat_dir) {
            // Portage leaves `-MERGING-…` directories behind while it works.
            if pf.starts_with('-') {
                continue;
            }
            let Some((pn, pvr)) = split_pf(&pf) else { continue };
            let origin = if dir.join("BINPKGMD5").exists() || dir.join("BUILD_ID").exists() {
                Origin::Binhost
            } else if pn.ends_with("-bin") {
                Origin::Prebuilt
            } else if installs_machine_code(&dir) {
                Origin::Compiled
            } else {
                Origin::Noarch
            };
            let cp = format!("{cat}/{pn}");
            pkgs.push(Pkg {
                world: world.contains(&cp),
                cat: cat.clone(),
                pn: pn.to_string(),
                pvr: pvr.to_string(),
                slot: field(&dir, "SLOT"),
                repo: field(&dir, "repository"),
                origin,
                build_time: field(&dir, "BUILD_TIME").parse().unwrap_or(0),
                size: field(&dir, "SIZE").parse().unwrap_or(0),
                eco: eco(&field(&dir, "INHERITED")),
                description: field(&dir, "DESCRIPTION"),
                homepage: field(&dir, "HOMEPAGE"),
                use_flags: field(&dir, "USE"),
            });
        }
    }
    Ok(pkgs)
}

/// Which packages a list shows.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum, Debug)]
pub enum Filter {
    All,
    /// Fetched as binary packages.
    Binhost,
    /// Built on this machine: compiled, prebuilt or noarch.
    Here,
    Compiled,
    Prebuilt,
    Noarch,
    /// Carrying local patches.
    Patched,
    /// From a repository other than gentoo.
    Overlay,
    /// Asked for by name: in the world file.
    World,
}

impl Filter {
    const ORDER: [Filter; 9] = [
        Filter::All,
        Filter::Binhost,
        Filter::Here,
        Filter::Compiled,
        Filter::Prebuilt,
        Filter::Noarch,
        Filter::Patched,
        Filter::Overlay,
        Filter::World,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Binhost => "binhost",
            Filter::Here => "built here",
            Filter::Compiled => "compiled",
            Filter::Prebuilt => "prebuilt",
            Filter::Noarch => "noarch",
            Filter::Patched => "patched",
            Filter::Overlay => "overlays",
            Filter::World => "world",
        }
    }

    pub fn step(self, by: isize) -> Filter {
        let i = Self::ORDER.iter().position(|&f| f == self).unwrap() as isize;
        let n = Self::ORDER.len() as isize;
        Self::ORDER[(i + by).rem_euclid(n) as usize]
    }

    pub fn keep(self, p: &Pkg, patched: bool) -> bool {
        match self {
            Filter::All => true,
            Filter::Binhost => p.origin == Origin::Binhost,
            Filter::Here => p.origin != Origin::Binhost,
            Filter::Compiled => p.origin == Origin::Compiled,
            Filter::Prebuilt => p.origin == Origin::Prebuilt,
            Filter::Noarch => p.origin == Origin::Noarch,
            Filter::Patched => patched,
            Filter::Overlay => p.repo != "gentoo",
            Filter::World => p.world,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
