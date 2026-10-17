//! mmm, Marko's Mail Machine: reading mail in the terminal, over notmuch. Tags it changes reach
//! Gmail through `deck-mail sync`, as anything else's do.

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
            "mmm [notmuch query]: read mail; ? in it explains what is selected, ? twice lists every key."
        );
        println!(
            "Searches along the top: {} (Name = query, a line each).",
            config::path().display()
        );
        return Ok(());
    }
    let query = (!args.is_empty()).then(|| args.join(" "));
    let mut app = app::App::new(query)?;
    let mut terminal = ratatui::init();
    let res = (|| -> Result<()> {
        while !app.quit {
            if let Some(d) = app.edit.take() {
                // The editor gets the terminal; mmm comes back when it exits.
                let line = d.write()?;
                ratatui::restore();
                let editor = std::env::var("VISUAL")
                    .or_else(|_| std::env::var("EDITOR"))
                    .unwrap_or_else(|_| "nvim".into());
                let ok = std::process::Command::new(&editor)
                    .arg(format!("+{line}"))
                    .arg(&d.file)
                    .status()
                    .is_ok_and(|s| s.success());
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
