//! What deck's programs share: which package owns a file, and reading processes from /proc.

pub mod pkgmap;
pub mod procfs;

use std::path::PathBuf;

/// Where deck-usaged writes, as root.
pub const USAGE_DB: &str = "/var/lib/deck/usage.db";

/// Where deck-focusd writes, as the user: $XDG_STATE_HOME/deck/focus.db.
pub fn focus_db() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("deck/focus.db")
}
