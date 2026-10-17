//! Address completion: people you wrote to, then people who wrote to you, from notmuch. The
//! editor asks through `mmm addresses PREFIX` (the draft's Tab on a To, Cc or Bcc line).
//!
//! Listing them takes notmuch a second, too slow for each key, so the list is kept in
//! ~/.cache/mmm/addresses and made again when the database has changed and it's 10 minutes old.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    name: String,
    address: String,
    #[serde(default)]
    count: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub name: String,
    pub address: String,
    pub score: u64,
}

impl Person {
    pub fn line(&self) -> String {
        if self.name.is_empty() {
            self.address.clone()
        } else if self.name.contains([',', '"', '<', '(', ';', ':', '@']) {
            format!("\"{}\" <{}>", self.name.replace('"', ""), self.address)
        } else {
            format!("{} <{}>", self.name, self.address)
        }
    }
}

fn cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"))
        .join("mmm/addresses")
}

fn found(args: &[&str]) -> Result<Vec<Found>> {
    let out = Command::new("notmuch")
        .args([
            "address",
            "--format=json",
            "--output=count",
            "--deduplicate=address",
        ])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("running notmuch address")?;
    if !out.status.success() {
        bail!("notmuch address: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    serde_json::from_slice(&out.stdout).context("reading notmuch address")
}

/// Mail no one answers: not worth offering.
fn robot(address: &str) -> bool {
    let local = address.split('@').next().unwrap_or("").to_ascii_lowercase();
    [
        "noreply",
        "no-reply",
        "donotreply",
        "do-not-reply",
        "notification",
        "mailer-daemon",
        "bounce",
    ]
    .iter()
    .any(|r| local.contains(r))
}

/// Everyone, best first: a mail you sent someone counts five times one they sent you.
fn gather(me: &[String]) -> Result<Vec<Person>> {
    let mut by: HashMap<String, Person> = HashMap::new();
    let lists = [
        (found(&["--output=recipients", "tag:sent"])?, 5),
        (
            found(&["--output=sender", "not tag:sent and not tag:spam and date:5y.."])?,
            1,
        ),
    ];
    for (list, weight) in lists {
        for f in list {
            let key = f.address.to_ascii_lowercase();
            if robot(&key) || me.contains(&key) || !key.contains('@') {
                continue;
            }
            let p = by.entry(key).or_insert_with(|| Person {
                name: String::new(),
                address: f.address.clone(),
                score: 0,
            });
            p.score += f.count * weight;
            // The name you used for them wins over the one they gave themselves.
            if p.name.is_empty() && f.name != f.address {
                p.name = f.name.trim().to_string();
            }
        }
    }
    let mut all: Vec<Person> = by.into_values().collect();
    all.sort_by(|a, b| b.score.cmp(&a.score).then(a.address.cmp(&b.address)));
    Ok(all)
}

fn my_addresses() -> Vec<String> {
    crate::compose::identities()
        .iter()
        .filter_map(|i| crate::compose::addresses(i).ok()?.into_iter().next())
        .map(|(_, a)| a.to_ascii_lowercase())
        .collect()
}

/// The cached list, made again when it's stale.
pub fn all() -> Result<Vec<Person>> {
    let path = cache();
    let lastmod = crate::notmuch::lastmod().unwrap_or_default();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let (head, body) = text.split_once('\n').unwrap_or(("", ""));
    let age = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .unwrap_or(Duration::MAX);
    if head == lastmod || (!head.is_empty() && age < Duration::from_secs(600)) {
        return Ok(parse(body));
    }
    let people = gather(&my_addresses())?;
    let mut out = format!("{lastmod}\n");
    for p in &people {
        out += &format!("{}\t{}\t{}\n", p.score, p.address, p.name);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(tmp, &path)?;
    Ok(people)
}

fn parse(body: &str) -> Vec<Person> {
    body.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            Some(Person {
                score: f.next()?.parse().ok()?,
                address: f.next()?.to_string(),
                name: f.next().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// The people whose name or address has a word starting with what was typed, every word of it.
pub fn complete(people: &[Person], typed: &str, limit: usize) -> Vec<Person> {
    let want: Vec<String> = typed.split_whitespace().map(str::to_lowercase).collect();
    if want.is_empty() {
        return vec![];
    }
    let words = |p: &Person| -> Vec<String> {
        format!("{} {}", p.name, p.address)
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    people
        .iter()
        .filter(|p| {
            let ws = words(p);
            let addr = p.address.to_lowercase();
            want.iter()
                .all(|w| addr.starts_with(w.as_str()) || ws.iter().any(|x| x.starts_with(w.as_str())))
        })
        .take(limit)
        .cloned()
        .collect()
}

/// `mmm addresses PREFIX`: a line each, for the editor.
pub fn print(typed: &str) -> Result<()> {
    for p in complete(&all()?, typed, 30) {
        println!("{}", p.line());
    }
    Ok(())
}

/// The editor's side: on a To, Cc or Bcc line, Tab completes the address being typed (another
/// Tab, the next one). For nvim and vim; sourced when the draft opens.
pub fn vim_script(mmm: &str) -> String {
    format!(
        r#"function! MmmAddresses(findstart, base) abort
  let line = getline('.')
  if a:findstart
    if line !~? '^\(to\|cc\|bcc\):'
      return -3
    endif
    let start = col('.') - 1
    while start > 0 && line[start - 1] !~ '[,:]'
      let start -= 1
    endwhile
    while line[start] == ' '
      let start += 1
    endwhile
    return start
  endif
  return systemlist({mmm} . ' addresses ' . shellescape(a:base))
endfunction
" In a function, not the mapping: a mapping would read \| in the pattern as |.
function! MmmTab() abort
  if pumvisible()
    return "\<C-n>"
  endif
  return getline('.') =~? '^\(to\|cc\|bcc\):' ? "\<C-x>\<C-o>" : "\<Tab>"
endfunction
setlocal omnifunc=MmmAddresses completeopt=menuone,noinsert
setlocal filetype=mail textwidth=0
inoremap <buffer> <expr> <Tab> MmmTab()
inoremap <buffer> <expr> <S-Tab> pumvisible() ? "\<C-p>" : "\<S-Tab>"
"#,
        mmm = vim_string(mmm)
    )
}

fn vim_string(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, address: &str, score: u64) -> Person {
        Person {
            name: name.into(),
            address: address.into(),
            score,
        }
    }

    #[test]
    fn completes_by_any_word() {
        let people = vec![
            p("Ana Horvat", "ana.horvat@example.org", 30),
            p("Šime Anić", "sime@example.hr", 20),
            p("", "anamarija@example.com", 5),
            p("Ivo", "ivo@example.org", 1),
        ];
        let got =
            |t: &str| -> Vec<String> { complete(&people, t, 10).into_iter().map(|p| p.address).collect() };
        assert_eq!(got("ana"), ["ana.horvat@example.org", "anamarija@example.com"]);
        assert_eq!(
            got("an"),
            [
                "ana.horvat@example.org",
                "sime@example.hr",
                "anamarija@example.com"
            ]
        );
        assert_eq!(got("šime"), ["sime@example.hr"]);
        assert_eq!(got("ana hor"), ["ana.horvat@example.org"]);
        assert_eq!(got("ivo@"), ["ivo@example.org"]);
        assert!(got("").is_empty());
    }

    #[test]
    fn lines_quote_names_that_need_it() {
        assert_eq!(p("Ana Horvat", "a@x", 0).line(), "Ana Horvat <a@x>");
        assert_eq!(p("Horvat, Ana", "a@x", 0).line(), "\"Horvat, Ana\" <a@x>");
        assert_eq!(p("", "a@x", 0).line(), "a@x");
        assert!(robot("no-reply@hep.hr") && robot("Notifications@github.com") && !robot("ana@x"));
    }

    #[test]
    fn the_cache_reads_back() {
        let people = parse("12\tana@x\tAna Horvat\n3\tivo@x\t\n");
        assert_eq!(people, [p("Ana Horvat", "ana@x", 12), p("", "ivo@x", 3)]);
    }
}
