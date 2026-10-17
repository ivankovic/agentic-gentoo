//! mmm, Marko's Mail Machine: reading mail in the terminal, over notmuch. Tags it changes reach
//! Gmail through `deck-mail sync`, as anything else's do.

mod app;
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
            terminal.draw(|f| ui::draw(f, &mut app))?;
            // New mail or tags from the sync timer show up within a couple of seconds.
            if event::poll(Duration::from_secs(2))? {
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
