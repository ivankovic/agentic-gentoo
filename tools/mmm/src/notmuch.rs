//! notmuch, through its command line and JSON output: searching threads, reading one, changing
//! tags, and fetching an attachment's bytes. mmm never writes mail files itself.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::process::{Command, Stdio};

fn notmuch(args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("notmuch")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("running notmuch")?;
    if !out.status.success() {
        bail!(
            "notmuch {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

/// A thread as `notmuch search` lists it.
#[derive(Deserialize, Clone, Debug)]
pub struct Thread {
    pub thread: String,
    pub timestamp: i64,
    pub matched: u32,
    pub total: u32,
    pub authors: String,
    #[serde(default)]
    pub subject: String,
    pub tags: Vec<String>,
}

impl Thread {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

/// Threads matching `query`, newest first, `limit` of them from `offset`.
pub fn search(query: &str, offset: usize, limit: usize) -> Result<Vec<Thread>> {
    let out = notmuch(&[
        "search",
        "--format=json",
        "--output=summary",
        "--sort=newest-first",
        &format!("--offset={offset}"),
        &format!("--limit={limit}"),
        query,
    ])?;
    serde_json::from_slice(&out).context("reading notmuch search")
}

pub fn count(query: &str) -> Result<u64> {
    let out = notmuch(&["count", query])?;
    String::from_utf8_lossy(&out)
        .trim()
        .parse()
        .context("reading notmuch count")
}

/// The database's revision: it changes whenever mail or tags change.
pub fn lastmod() -> Result<String> {
    let out = notmuch(&["count", "--lastmod", "*"])?;
    Ok(String::from_utf8_lossy(&out)
        .split_whitespace()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" "))
}

/// The ids of the messages matching `query`, deleted and spam ones included.
pub fn message_ids(query: &str) -> Result<Vec<String>> {
    let out = notmuch(&["search", "--exclude=false", "--output=messages", query])?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| l.strip_prefix("id:"))
        .map(str::to_string)
        .collect())
}

/// Adds (`+tag`) or removes (`-tag`) one tag on these messages.
pub fn tag(ids: &[String], change: &str) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let query: Vec<String> = ids.iter().map(|id| format!("id:{}", quote(id))).collect();
    let mut args = vec!["tag", change, "--"];
    let joined = query.join(" or ");
    args.push(&joined);
    notmuch(&args).map(|_| ())
}

/// A message id quoted for a notmuch query, should it hold spaces or quotes.
fn quote(id: &str) -> String {
    if id
        .chars()
        .any(|c| c.is_whitespace() || c == '"' || c == '(' || c == ')')
    {
        format!("\"{}\"", id.replace('"', "\"\""))
    } else {
        id.to_string()
    }
}

/// One message of a thread, with its body parts as notmuch decoded them.
#[derive(Clone, Debug)]
pub struct Message {
    pub id: String,
    pub timestamp: i64,
    pub tags: Vec<String>,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: Vec<Value>,
}

impl Message {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

/// Every message of a thread, in thread order, replies after what they answer.
pub fn show_thread(thread: &str) -> Result<Vec<Message>> {
    let out = notmuch(&[
        "show",
        "--format=json",
        "--include-html",
        "--exclude=false",
        "--entire-thread=true",
        &format!("thread:{thread}"),
    ])?;
    let v: Value = serde_json::from_slice(&out).context("reading notmuch show")?;
    let mut msgs = vec![];
    // [thread] → thread: [node] → node: [message, [node]]
    for t in v.as_array().into_iter().flatten() {
        for node in t.as_array().into_iter().flatten() {
            walk(node, &mut msgs);
        }
    }
    Ok(msgs)
}

fn walk(node: &Value, out: &mut Vec<Message>) {
    let Some(pair) = node.as_array() else { return };
    if let Some(m) = pair.first().filter(|m| m.is_object()) {
        let h = &m["headers"];
        let s = |k: &str| h[k].as_str().unwrap_or_default().to_string();
        out.push(Message {
            id: m["id"].as_str().unwrap_or_default().to_string(),
            timestamp: m["timestamp"].as_i64().unwrap_or_default(),
            tags: m["tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect(),
            from: s("From"),
            to: s("To"),
            cc: s("Cc"),
            subject: s("Subject"),
            body: m["body"].as_array().cloned().unwrap_or_default(),
        });
    }
    for child in pair.get(1).and_then(Value::as_array).into_iter().flatten() {
        walk(child, out);
    }
}

/// The decoded bytes of one part of a message, an attachment.
pub fn part(id: &str, part: u64) -> Result<Vec<u8>> {
    notmuch(&[
        "show",
        "--format=raw",
        &format!("--part={part}"),
        &format!("id:{}", quote(id)),
    ])
}
