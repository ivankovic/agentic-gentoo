//! mmm, Marko's Mail Machine: reading mail in the terminal, over notmuch. Tags it changes reach
//! Gmail through `deck-mail sync`, as anything else's do.

mod addresses;
mod app;
mod archive;
mod compose;
mod config;
mod notmuch;
mod render;
mod ui;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::time::Duration;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "mmm [notmuch query]: read and write mail; ? in it explains what is selected, ? twice lists every key.\nmmm addresses PREFIX: the people to complete an address with, best first (the editor's Tab)."
        );
        println!(
            "Searches along the top: {} (Name = query, a line each).",
            config::path().display()
        );
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("addresses") {
        return addresses::print(&args[1..].join(" "));
    }
    let query = (!args.is_empty()).then(|| args.join(" "));
    let mut app = app::App::new(query)?;
    let mut terminal = ratatui::init();
    let res = (|| -> Result<()> {
        while !app.quit {
            if let Some(d) = app.edit.take() {
                // The editor gets the terminal; mmm comes back when it exits. The draft is kept
                // first, so it's in Drafts should mmm not come back.
                let line = d.write()?;
                let _ = d.save();
                ratatui::restore();
                let editor = std::env::var("VISUAL")
                    .or_else(|_| std::env::var("EDITOR"))
                    .unwrap_or_else(|_| "nvim".into());
                let mut cmd = std::process::Command::new(&editor);
                cmd.arg(format!("+{line}"));
                // nvim and vim: Tab completes addresses on the To, Cc and Bcc lines.
                if let Some(script) = editor_script(&editor) {
                    cmd.arg("-c").arg(format!("source {}", script.display()));
                }
                let ok = cmd.arg(&d.file).status().is_ok_and(|s| s.success());
                terminal = ratatui::init();
                terminal.clear()?;
                app.edited(d, ok);
            }
            terminal.draw(|f| ui::draw(f, &mut app))?;
            // New mail or tags from the sync timer show up within a couple of seconds; a mail
            // in its minute counts down by the second.
            let wait = if app.pending.is_empty() { 2 } else { 1 };
            if event::poll(Duration::from_secs(wait))? {
                if let Event::Key(k) = event::read()?
                    && k.kind == KeyEventKind::Press
                {
                    app.key(k);
                }
            } else {
                app.tick();
            }
        }
        Ok(())
    })();
    ratatui::restore();
    res
}

/// The editor's address completion, written where the editor can source it, for nvim and vim.
fn editor_script(editor: &str) -> Option<std::path::PathBuf> {
    let name = std::path::Path::new(editor.split_whitespace().next()?)
        .file_name()?
        .to_str()?;
    if !matches!(name, "nvim" | "vim" | "vi") {
        return None;
    }
    let me = std::env::current_exe().ok()?;
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("mmm");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("draft.vim");
    std::fs::write(&path, addresses::vim_script(&me.to_string_lossy())).ok()?;
    Some(path)
}
