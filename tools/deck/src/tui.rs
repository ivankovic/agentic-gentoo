//! The interactive view: a package list and a patch list, with a detail pane under each.

use crate::model::{Model, date};
use crate::patches::Kind;
use crate::track::State;
use crate::vdb::{Filter, Origin};
use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Tabs, Wrap,
};
use ratatui::{DefaultTerminal, Frame};

#[derive(Clone, Copy, PartialEq)]
enum View {
    Packages,
    Patches,
}

/// What happens when a text prompt is confirmed.
enum Then {
    Url(State),
    Note(State, String),
    Upstream,
}

enum Mode {
    Normal,
    Search,
    Pick(ListState),
    Input {
        prompt: &'static str,
        buf: String,
        then: Then,
    },
}

struct App {
    m: Model,
    view: View,
    filter: Filter,
    query: String,
    pkg_rows: Vec<usize>,
    patch_rows: Vec<usize>,
    pkg_tab: TableState,
    patch_tab: TableState,
    mode: Mode,
    status: String,
    quit: bool,
}

pub fn run(m: Model) -> Result<()> {
    let mut app = App {
        m,
        view: View::Packages,
        filter: Filter::All,
        query: String::new(),
        pkg_rows: vec![],
        patch_rows: vec![],
        pkg_tab: TableState::default().with_selected(0),
        patch_tab: TableState::default().with_selected(0),
        mode: Mode::Normal,
        status: String::new(),
        quit: false,
    };
    app.refilter();
    let mut terminal = ratatui::init();
    let res = app.run(&mut terminal);
    ratatui::restore();
    res
}

fn origin_color(o: Origin) -> Color {
    match o {
        Origin::Binhost => Color::DarkGray,
        Origin::Compiled => Color::Yellow,
        Origin::Prebuilt => Color::Magenta,
        Origin::Noarch => Color::Cyan,
    }
}

fn state_color(label: &str) -> Color {
    match label {
        "untracked" | "unsent" | "changes-requested" => Color::Yellow,
        "sent" | "in-review" | "accepted" => Color::Blue,
        "released" | "backport" | "dropped" => Color::Green,
        "rejected" => Color::Red,
        _ => Color::DarkGray,
    }
}

fn step(state: &mut TableState, len: usize, by: isize) {
    if len == 0 {
        state.select(None);
        return;
    }
    let cur = state.selected().unwrap_or(0) as isize;
    state.select(Some((cur + by).clamp(0, len as isize - 1) as usize));
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Length(h)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(w.min(area.width))])
        .flex(Flex::Center)
        .areas(v);
    r
}

fn kv<'a>(k: &'a str, v: impl Into<String>) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{k:<10}"), Style::new().dark_gray()),
        Span::raw(v.into()),
    ])
}

impl App {
    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                self.key(k);
            }
        }
        Ok(())
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.pkg_rows = (0..self.m.pkgs.len())
            .filter(|&i| {
                let p = &self.m.pkgs[i];
                self.filter.keep(p, self.m.patch_counts[i] > 0) && p.atom().to_lowercase().contains(&q)
            })
            .collect();
        self.patch_rows = (0..self.m.rows.len())
            .filter(|&i| self.m.rows[i].id.to_lowercase().contains(&q))
            .collect();
        step(&mut self.pkg_tab, self.pkg_rows.len(), 0);
        step(&mut self.patch_tab, self.patch_rows.len(), 0);
    }

    fn selected_patch(&self) -> Option<usize> {
        self.patch_tab
            .selected()
            .and_then(|i| self.patch_rows.get(i).copied())
    }

    fn selected_pkg(&self) -> Option<usize> {
        self.pkg_tab
            .selected()
            .and_then(|i| self.pkg_rows.get(i).copied())
    }

    fn reload(&mut self) {
        match Model::load(self.m.host_dir.clone()) {
            Ok(m) => {
                self.m = m;
                self.refilter();
                self.status = "reloaded".into();
            }
            Err(e) => self.status = format!("reload failed: {e:#}"),
        }
    }

    fn key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.key_normal(k),
            Mode::Search => match k.code {
                KeyCode::Enter => {}
                KeyCode::Esc => {
                    self.query.clear();
                    self.refilter();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    self.refilter();
                    self.mode = Mode::Search;
                }
                KeyCode::Char(c) => {
                    self.query.push(c);
                    self.refilter();
                    self.mode = Mode::Search;
                }
                _ => self.mode = Mode::Search,
            },
            Mode::Pick(mut list) => match k.code {
                KeyCode::Esc | KeyCode::Char('q') => self.status = "cancelled".into(),
                KeyCode::Down | KeyCode::Char('j') => {
                    list.select_next();
                    self.mode = Mode::Pick(list);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    list.select_previous();
                    self.mode = Mode::Pick(list);
                }
                KeyCode::Enter => {
                    let state = State::ALL[list.selected().unwrap_or(0).min(State::ALL.len() - 1)];
                    self.mode = Mode::Input {
                        prompt: "URL (PR, post, commit; Enter to skip)",
                        buf: String::new(),
                        then: Then::Url(state),
                    };
                }
                _ => self.mode = Mode::Pick(list),
            },
            Mode::Input {
                prompt,
                mut buf,
                then,
            } => match k.code {
                KeyCode::Esc => self.status = "cancelled".into(),
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Input { prompt, buf, then };
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Input { prompt, buf, then };
                }
                KeyCode::Enter => self.confirm(buf, then),
                _ => self.mode = Mode::Input { prompt, buf, then },
            },
        }
    }

    fn confirm(&mut self, buf: String, then: Then) {
        let Some(row) = self.selected_patch() else { return };
        let id = self.m.rows[row].id.clone();
        let res = match then {
            Then::Url(state) => {
                self.mode = Mode::Input {
                    prompt: "Note (Enter to skip)",
                    buf: String::new(),
                    then: Then::Note(state, buf.trim().to_string()),
                };
                return;
            }
            Then::Note(state, url) => self
                .m
                .record(&id, state, url, buf.trim().to_string())
                .map(|_| format!("{id}: {}", state.label())),
            Then::Upstream => self
                .m
                .set_upstream(&id, buf.trim().to_string())
                .map(|_| format!("{id}: upstream set")),
        };
        self.status = match res {
            Ok(s) => format!("saved · {s}"),
            Err(e) => format!("not saved: {e:#}"),
        };
        self.refilter();
    }

    fn key_normal(&mut self, k: KeyEvent) {
        let (tab, len) = match self.view {
            View::Packages => (&mut self.pkg_tab, self.pkg_rows.len()),
            View::Patches => (&mut self.patch_tab, self.patch_rows.len()),
        };
        self.status.clear();
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Tab | KeyCode::BackTab => {
                self.view = if self.view == View::Packages {
                    View::Patches
                } else {
                    View::Packages
                }
            }
            KeyCode::Char('1') => self.view = View::Packages,
            KeyCode::Char('2') => self.view = View::Patches,
            KeyCode::Down | KeyCode::Char('j') => step(tab, len, 1),
            KeyCode::Up | KeyCode::Char('k') => step(tab, len, -1),
            KeyCode::PageDown | KeyCode::Char('d') => step(tab, len, 15),
            KeyCode::PageUp | KeyCode::Char('u') => step(tab, len, -15),
            KeyCode::Home | KeyCode::Char('g') => step(tab, len, isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => step(tab, len, isize::MAX / 2),
            KeyCode::Char('/') => self.mode = Mode::Search,
            KeyCode::Esc => {
                self.query.clear();
                self.refilter();
            }
            KeyCode::Char('r') => self.reload(),
            KeyCode::Char('f') if self.view == View::Packages => {
                self.filter = self.filter.step(1);
                self.refilter();
            }
            KeyCode::Char('F') if self.view == View::Packages => {
                self.filter = self.filter.step(-1);
                self.refilter();
            }
            KeyCode::Enter if self.view == View::Packages => {
                if let Some(i) = self.selected_pkg()
                    && self.m.patch_counts[i] > 0
                {
                    self.query = self.m.pkgs[i].cp();
                    self.view = View::Patches;
                    self.refilter();
                }
            }
            KeyCode::Char('m') | KeyCode::Enter if self.view == View::Patches => {
                if let Some(r) = self.selected_patch() {
                    let cur = self.m.last_event(&self.m.rows[r]).map(|e| e.state);
                    let at = cur
                        .and_then(|s| State::ALL.iter().position(|&x| x == s))
                        .unwrap_or(0);
                    self.mode = Mode::Pick(ListState::default().with_selected(Some(at)));
                }
            }
            KeyCode::Char('U') if self.view == View::Patches => {
                if let Some(r) = self.selected_patch() {
                    let buf = self
                        .m
                        .tracked(&self.m.rows[r])
                        .map(|t| t.upstream.clone())
                        .unwrap_or_default();
                    self.mode = Mode::Input {
                        prompt: "Upstream project URL",
                        buf,
                        then: Then::Upstream,
                    };
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let [top, body, detail, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(12),
            Constraint::Length(1),
        ])
        .areas(f.area());

        let [tabs, summary] = Layout::horizontal([Constraint::Length(26), Constraint::Fill(1)]).areas(top);
        f.render_widget(
            Tabs::new(["1 Packages", "2 Patches"])
                .select(if self.view == View::Packages { 0 } else { 1 })
                .highlight_style(Style::new().bold().reversed()),
            tabs,
        );
        f.render_widget(Line::from(self.summary()).right_aligned(), summary);

        match self.view {
            View::Packages => self.draw_packages(f, body, detail),
            View::Patches => self.draw_patches(f, body, detail),
        }

        let footer = match &self.mode {
            Mode::Search => Line::from(vec![Span::raw("/").bold(), Span::raw(self.query.clone())]),
            _ if !self.status.is_empty() => Line::from(self.status.clone()).yellow(),
            _ => Line::from(match self.view {
                View::Packages => {
                    "q quit · Tab view · j/k move · / search · f/F filter · Enter patches · r reload"
                }
                View::Patches => {
                    "q quit · Tab view · j/k move · / search · m change state · U upstream · r reload"
                }
            })
            .dark_gray(),
        };
        f.render_widget(footer, foot);

        match &mut self.mode {
            Mode::Pick(list) => {
                let area = centered(f.area(), 72, State::ALL.len() as u16 + 2);
                let items: Vec<ListItem> = State::ALL
                    .iter()
                    .map(|s| {
                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("{:<18}", s.label()),
                                Style::new().fg(state_color(s.label())),
                            ),
                            Span::raw(s.help()).dark_gray(),
                        ]))
                    })
                    .collect();
                f.render_widget(Clear, area);
                f.render_stateful_widget(
                    List::new(items)
                        .block(Block::bordered().title(" New state "))
                        .highlight_style(Style::new().reversed()),
                    area,
                    list,
                );
            }
            Mode::Input { prompt, buf, .. } => {
                let area = centered(f.area(), 80, 3);
                f.render_widget(Clear, area);
                f.render_widget(
                    Paragraph::new(format!("{buf}▏")).block(Block::bordered().title(format!(" {prompt} "))),
                    area,
                );
            }
            _ => {}
        }
    }

    fn summary(&self) -> Vec<Span<'static>> {
        let count = |o: Origin| self.m.pkgs.iter().filter(|p| p.origin == o).count();
        let mut spans = vec![Span::raw(format!("{} installed", self.m.pkgs.len()))];
        for o in [
            Origin::Binhost,
            Origin::Compiled,
            Origin::Prebuilt,
            Origin::Noarch,
        ] {
            spans.push(Span::raw(" · "));
            spans.push(Span::styled(
                format!("{} {}", count(o), o.label()),
                Style::new().fg(origin_color(o)),
            ));
        }
        let open = self
            .m
            .rows
            .iter()
            .filter(|r| self.m.last_event(r).is_none_or(|e| e.state.open()))
            .count();
        spans.push(Span::raw(format!(
            " · {} patches, {open} open ",
            self.m.rows.len()
        )));
        spans
    }

    fn title(&self, what: &str, shown: usize, total: usize) -> String {
        let mut t = format!(" {what} · {shown} of {total}");
        if what == "Packages" {
            t += &format!(" · filter: {}", self.filter.label());
        }
        if !self.query.is_empty() {
            t += &format!(" · /{}", self.query);
        }
        t + " "
    }

    fn draw_packages(&mut self, f: &mut Frame, body: Rect, detail: Rect) {
        let rows = self.pkg_rows.iter().map(|&i| {
            let p = &self.m.pkgs[i];
            let n = self.m.patch_counts[i];
            Row::new(vec![
                Cell::from(p.atom()),
                Cell::from(p.origin.label()).fg(origin_color(p.origin)),
                Cell::from(p.repo.clone()).fg(if p.repo == "gentoo" {
                    Color::Reset
                } else {
                    Color::Green
                }),
                Cell::from(p.eco),
                Cell::from(if p.world { "★" } else { "" }),
                Cell::from(if n > 0 { n.to_string() } else { String::new() }).yellow(),
                Cell::from(date(p.build_time)).dark_gray(),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Fill(1),
                Constraint::Length(8),
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Length(5),
                Constraint::Length(7),
                Constraint::Length(10),
            ],
        )
        .header(Row::new(["Package", "Origin", "Repo", "Eco", "World", "Patches", "Built"]).bold())
        .block(Block::bordered().title(self.title("Packages", self.pkg_rows.len(), self.m.pkgs.len())))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        f.render_stateful_widget(table, body, &mut self.pkg_tab);

        let mut lines = vec![];
        if let Some(i) = self.selected_pkg() {
            let p = &self.m.pkgs[i];
            lines.push(Line::from(vec![
                Span::raw(p.atom()).bold(),
                Span::raw(format!("  {}", p.description)),
            ]));
            lines.push(kv(
                "origin",
                format!("{} — {}", p.origin.label(), p.origin.explain()),
            ));
            lines.push(kv(
                "repo",
                format!(
                    "{}   slot {}   {}",
                    p.repo,
                    p.slot,
                    if p.world { "in world" } else { "dependency" }
                ),
            ));
            lines.push(kv(
                "built",
                format!("{}   {:.1} MiB", date(p.build_time), p.size as f64 / 1048576.0),
            ));
            lines.push(kv("homepage", p.homepage.clone()));
            for r in self
                .m
                .rows
                .iter()
                .filter(|r| r.file.is_some_and(|f| self.m.files[f].applies == Some(i)))
            {
                lines.push(Line::from(vec![
                    Span::styled(format!("{:<10}", "patch"), Style::new().dark_gray()),
                    Span::styled(
                        format!("{:<18}", self.m.state_label(r)),
                        Style::new().fg(state_color(self.m.state_label(r))),
                    ),
                    Span::raw(r.id.rsplit('/').next().unwrap_or("").to_string()),
                ]));
            }
            lines.push(kv("USE", p.use_flags.clone()));
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::bordered()),
            detail,
        );
    }

    fn draw_patches(&mut self, f: &mut Frame, body: Rect, detail: Rect) {
        let rows = self.patch_rows.iter().map(|&i| {
            let r = &self.m.rows[i];
            let state = self.m.state_label(r);
            let file = r.file.map(|i| &self.m.files[i]);
            let (pkg, name) = r.id.rsplit_once('/').unwrap_or(("", &r.id));
            Row::new(vec![
                Cell::from(state).fg(state_color(state)),
                Cell::from(pkg.to_string()),
                Cell::from(name.to_string()),
                Cell::from(self.m.applies_label(r)),
                Cell::from(match file {
                    Some(f) if f.deployed => Span::raw("yes"),
                    Some(_) => Span::raw("no").red(),
                    None => Span::raw(""),
                }),
                Cell::from(
                    self.m
                        .last_event(r)
                        .map_or(String::new(), |e| e.at.chars().take(10).collect()),
                )
                .dark_gray(),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(17),
                Constraint::Fill(1),
                Constraint::Fill(2),
                Constraint::Fill(1),
                Constraint::Length(8),
                Constraint::Length(10),
            ],
        )
        .header(Row::new(["State", "Package", "Patch", "Applies to", "Deployed", "Since"]).bold())
        .block(Block::bordered().title(self.title("Patches", self.patch_rows.len(), self.m.rows.len())))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        f.render_stateful_widget(table, body, &mut self.patch_tab);

        let mut lines = vec![];
        if let Some(i) = self.selected_patch() {
            let r = &self.m.rows[i];
            let file = r.file.map(|i| &self.m.files[i]);
            lines.push(Line::from(r.id.clone()).bold());
            if let Some(f) = file {
                if !f.subject.is_empty() {
                    lines.push(kv("subject", f.subject.clone()));
                }
                let kind = match f.kind {
                    Kind::User => "user patch",
                    Kind::Overlay => "local overlay ebuild's patch",
                };
                lines.push(kv("file", format!("{} ({kind})", f.rel)));
            } else {
                lines.push(kv(
                    "file",
                    "no longer in the host directory; kept for its history",
                ));
            }
            let upstream = self.m.tracked(r).map(|t| t.upstream.clone()).unwrap_or_default();
            lines.push(kv(
                "upstream",
                if upstream.is_empty() {
                    "(not set — U to set)".into()
                } else {
                    upstream
                },
            ));
            match self.m.tracked(r) {
                Some(t) if !t.events.is_empty() => {
                    for e in t.events.iter().rev() {
                        let mut spans = vec![
                            Span::raw(format!("{}  ", e.at)).dark_gray(),
                            Span::styled(
                                format!("{:<18}", e.state.label()),
                                Style::new().fg(state_color(e.state.label())),
                            ),
                        ];
                        if !e.url.is_empty() {
                            spans.push(Span::raw(format!("{}  ", e.url)).underlined());
                        }
                        spans.push(Span::raw(e.note.clone()));
                        lines.push(Line::from(spans));
                    }
                }
                _ => lines.push(Line::from("not tracked yet — m to record its state").dark_gray()),
            }
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::bordered()),
            detail,
        );
    }
}
