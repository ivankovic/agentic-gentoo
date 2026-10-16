//! The interactive view: packages, patches and usage, with a detail pane under each.

use crate::model::{Model, date};
use crate::patches::Kind;
use crate::track::State;
use crate::usage::{self, Usage, pct};
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
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Packages,
    Patches,
    Usage,
}

/// The order of the package and usage lists.
#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Name,
    Cpu,
    Active,
    Memory,
}

impl Sort {
    fn label(self) -> &'static str {
        match self {
            Sort::Name => "name",
            Sort::Cpu => "CPU",
            Sort::Active => "active time",
            Sort::Memory => "peak memory",
        }
    }

    fn next(self) -> Sort {
        match self {
            Sort::Name => Sort::Cpu,
            Sort::Cpu => Sort::Active,
            Sort::Active => Sort::Memory,
            Sort::Memory => Sort::Name,
        }
    }

    fn key(self, r: Option<&usage::Row>) -> i64 {
        let Some(r) = r else { return 0 };
        match self {
            Sort::Name => 0,
            Sort::Cpu => r.cpu_us,
            Sort::Active => r.active_s,
            Sort::Memory => r.rss_peak_kb,
        }
    }
}

const PERIODS: [f64; 3] = [1.0, 7.0, 30.0];

/// The usage detail of one label: whose code it ran, who ran its code, its latest spikes.
type Detail = (String, Vec<(String, i64)>, Vec<(String, i64)>, Vec<usage::Spike>);

fn dur(secs: i64) -> String {
    match secs {
        0 => String::new(),
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
    }
}

fn mib(kb: i64) -> String {
    if kb == 0 {
        String::new()
    } else {
        format!("{:.0}", kb as f64 / 1024.0)
    }
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
    usage_db: PathBuf,
    usage: Option<Usage>,
    usage_err: String,
    /// Label → index into usage.rows.
    usage_ix: HashMap<String, usize>,
    usage_rows: Vec<usize>,
    usage_tab: TableState,
    period: usize,
    sort: Sort,
    detail: Option<Detail>,
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
        usage_db: std::env::var_os("DECK_USAGE_DB")
            .map_or_else(|| PathBuf::from(deck::USAGE_DB), PathBuf::from),
        usage: None,
        usage_err: String::new(),
        usage_ix: HashMap::new(),
        usage_rows: vec![],
        usage_tab: TableState::default().with_selected(0),
        period: 1,
        sort: Sort::Name,
        detail: None,
    };
    app.load_usage();
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

    fn load_usage(&mut self) {
        self.detail = None;
        match usage::load(&self.usage_db, PERIODS[self.period]) {
            Ok(u) => {
                self.usage_ix = u
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| (r.label.clone(), i))
                    .collect();
                self.usage = Some(u);
                self.usage_err.clear();
            }
            Err(e) => {
                self.usage = None;
                self.usage_ix.clear();
                self.usage_err = format!("{e:#}");
            }
        }
    }

    fn usage_of(&self, label: &str) -> Option<&usage::Row> {
        let u = self.usage.as_ref()?;
        self.usage_ix.get(label).map(|&i| &u.rows[i])
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.pkg_rows = (0..self.m.pkgs.len())
            .filter(|&i| {
                let p = &self.m.pkgs[i];
                self.filter.keep(p, self.m.patch_counts[i] > 0) && p.atom().to_lowercase().contains(&q)
            })
            .collect();
        if self.sort != Sort::Name {
            let keys: Vec<i64> = self
                .m
                .pkgs
                .iter()
                .map(|p| self.sort.key(self.usage_of(&p.cp())))
                .collect();
            self.pkg_rows
                .sort_by(|&a, &b| keys[b].cmp(&keys[a]).then(a.cmp(&b)));
        }
        self.patch_rows = (0..self.m.rows.len())
            .filter(|&i| self.m.rows[i].id.to_lowercase().contains(&q))
            .collect();
        self.usage_rows = match &self.usage {
            Some(u) => {
                let mut v: Vec<usize> = (0..u.rows.len())
                    .filter(|&i| u.rows[i].label.to_lowercase().contains(&q))
                    .collect();
                match self.sort {
                    Sort::Name => v.sort_by(|&a, &b| u.rows[a].label.cmp(&u.rows[b].label)),
                    s => v.sort_by(|&a, &b| {
                        s.key(Some(&u.rows[b]))
                            .cmp(&s.key(Some(&u.rows[a])))
                            .then(a.cmp(&b))
                    }),
                }
                v
            }
            None => vec![],
        };
        step(&mut self.pkg_tab, self.pkg_rows.len(), 0);
        step(&mut self.patch_tab, self.patch_rows.len(), 0);
        step(&mut self.usage_tab, self.usage_rows.len(), 0);
    }

    fn selected_usage(&self) -> Option<&usage::Row> {
        let u = self.usage.as_ref()?;
        self.usage_tab
            .selected()
            .and_then(|i| self.usage_rows.get(i))
            .map(|&i| &u.rows[i])
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
                self.load_usage();
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
            View::Usage => (&mut self.usage_tab, self.usage_rows.len()),
        };
        self.status.clear();
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Tab => {
                self.view = match self.view {
                    View::Packages => View::Patches,
                    View::Patches => View::Usage,
                    View::Usage => View::Packages,
                }
            }
            KeyCode::BackTab => {
                self.view = match self.view {
                    View::Packages => View::Usage,
                    View::Patches => View::Packages,
                    View::Usage => View::Patches,
                }
            }
            KeyCode::Char('1') => self.view = View::Packages,
            KeyCode::Char('2') => self.view = View::Patches,
            KeyCode::Char('3') => self.view = View::Usage,
            KeyCode::Char('s') if self.view != View::Patches => {
                self.sort = self.sort.next();
                self.refilter();
                self.status = format!("sorted by {}", self.sort.label());
            }
            KeyCode::Char('p') if self.view != View::Patches => {
                self.period = (self.period + 1) % PERIODS.len();
                self.load_usage();
                self.refilter();
                self.status = format!("usage over the last {} days", PERIODS[self.period]);
            }
            KeyCode::Enter if self.view == View::Usage => {
                if let Some(r) = self.selected_usage()
                    && r.label.contains('/')
                    && !r.label.contains(':')
                {
                    self.query = r.label.clone();
                    self.filter = Filter::All;
                    self.view = View::Packages;
                    self.refilter();
                }
            }
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

        let [tabs, summary] = Layout::horizontal([Constraint::Length(36), Constraint::Fill(1)]).areas(top);
        f.render_widget(
            Tabs::new(["1 Packages", "2 Patches", "3 Usage"])
                .select(match self.view {
                    View::Packages => 0,
                    View::Patches => 1,
                    View::Usage => 2,
                })
                .highlight_style(Style::new().bold().reversed()),
            tabs,
        );
        f.render_widget(Line::from(self.summary()).right_aligned(), summary);

        match self.view {
            View::Packages => self.draw_packages(f, body, detail),
            View::Patches => self.draw_patches(f, body, detail),
            View::Usage => self.draw_usage(f, body, detail),
        }

        let footer = match &self.mode {
            Mode::Search => Line::from(vec![Span::raw("/").bold(), Span::raw(self.query.clone())]),
            _ if !self.status.is_empty() => Line::from(self.status.clone()).yellow(),
            _ => Line::from(match self.view {
                View::Packages => {
                    "q quit · Tab view · j/k move · / search · f/F filter · s sort · p period · Enter patches · r reload"
                }
                View::Usage => "q quit · Tab view · j/k move · / search · s sort · p period · Enter package · r reload",
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
        if what != "Patches" {
            t += &format!(" · {} days · sort: {}", PERIODS[self.period], self.sort.label());
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
                Cell::from(match (self.usage_of(&p.cp()), &self.usage) {
                    (Some(r), Some(u)) if r.cpu_us > 0 => format!("{:.1}%", pct(r.cpu_us, u.total_cpu_us)),
                    _ => String::new(),
                }),
                Cell::from(self.usage_of(&p.cp()).map_or(String::new(), |r| dur(r.active_s))),
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
                Constraint::Length(6),
                Constraint::Length(6),
            ],
        )
        .header(
            Row::new([
                "Package", "Origin", "Repo", "Eco", "World", "Patches", "Built", "CPU", "Active",
            ])
            .bold(),
        )
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
            if let (Some(r), Some(u)) = (self.usage_of(&p.cp()), &self.usage) {
                lines.push(kv(
                    "usage",
                    format!(
                        "CPU {} ({:.1}%) · code {:.1}% · active {} · peak {} MiB · {} spikes   (3 for more)",
                        dur(r.cpu_us / 1_000_000),
                        pct(r.cpu_us, u.total_cpu_us),
                        pct(r.code_samples, u.total_samples),
                        dur(r.active_s),
                        mib(r.rss_peak_kb),
                        r.spikes
                    ),
                ));
            }
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

    fn draw_usage(&mut self, f: &mut Frame, body: Rect, detail: Rect) {
        let Some(u) = &self.usage else {
            f.render_widget(
                Paragraph::new(vec![
                    Line::from("No usage data.").bold(),
                    Line::from(self.usage_err.clone()),
                    Line::from(""),
                    Line::from("deck-usaged records CPU and memory (a root service), deck-focusd active time (a user service)."),
                ])
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(" Usage ")),
                body,
            );
            return;
        };
        let rows = self.usage_rows.iter().map(|&i| {
            let r = &u.rows[i];
            let installed = r.label.contains('/') && !r.label.contains(':');
            Row::new(vec![
                Cell::from(r.label.clone()).fg(if installed { Color::Reset } else { Color::Cyan }),
                Cell::from(dur(r.cpu_us / 1_000_000)),
                Cell::from(format!("{:.1}%", pct(r.cpu_us, u.total_cpu_us))),
                Cell::from(if r.code_samples > 0 {
                    format!("{:.1}%", pct(r.code_samples, u.total_samples))
                } else {
                    String::new()
                }),
                Cell::from(dur(r.active_s)),
                Cell::from(mib(r.rss_peak_kb)),
                Cell::from(mib(r.pss_avg_kb)),
                Cell::from(dur(r.hot_s)),
                Cell::from(if r.spikes > 0 {
                    r.spikes.to_string()
                } else {
                    String::new()
                })
                .yellow(),
                Cell::from(r.ctx.first().map_or(String::new(), |c| c.0.clone())).dark_gray(),
            ])
        });
        let mut title = self.title("Usage", self.usage_rows.len(), u.rows.len());
        if let Some(t) = u.started {
            title += &format!("· recording since {} ", usage::when(t));
        }
        if !u.focus {
            title += "· no active time (deck-focusd) ";
        }
        let table = Table::new(
            rows,
            [
                Constraint::Fill(1),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(6),
            ],
        )
        .header(
            Row::new([
                "Package or program",
                "CPU",
                "Share",
                "Code",
                "Active",
                "Peak",
                "PSS",
                ">1 core",
                "Spikes",
                "Ctx",
            ])
            .bold(),
        )
        .block(Block::bordered().title(title))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        f.render_stateful_widget(table, body, &mut self.usage_tab);

        let Some(r) = self.selected_usage().cloned() else {
            f.render_widget(Block::bordered(), detail);
            return;
        };
        if self.detail.as_ref().is_none_or(|d| d.0 != r.label) {
            let days = PERIODS[self.period];
            let (runs, ran_by) = usage::code(&self.usage_db, days, &r.label).unwrap_or_default();
            let spikes = usage::spikes(&self.usage_db, days, Some(&r.label)).unwrap_or_default();
            self.detail = Some((r.label.clone(), runs, ran_by, spikes));
        }
        let (_, runs, ran_by, spikes) = self.detail.as_ref().unwrap();
        let share = |v: &Vec<(String, i64)>| {
            let total: i64 = v.iter().map(|x| x.1).sum();
            v.iter()
                .take(5)
                .map(|(l, n)| format!("{l} {:.0}%", pct(*n, total)))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let mut lines = vec![
            Line::from(r.label.clone()).bold(),
            kv("contexts", usage::ctx_summary(&r) + "  (CPU seconds)"),
            kv("launches", format!("{}   alive {}", r.launches, dur(r.alive_s))),
        ];
        if !runs.is_empty() {
            lines.push(kv("runs code", share(runs)));
        }
        if !ran_by.is_empty() {
            lines.push(kv("code run by", share(ran_by)));
        }
        for s in spikes.iter().take(5) {
            lines.push(kv(
                "spike",
                format!(
                    "{}  {}s  {:.1} CPU-s  peak {:.1} cores  {} MiB  {}",
                    usage::when(s.start),
                    s.secs,
                    s.cpu_us as f64 / 1e6,
                    s.peak_cores,
                    mib(s.rss_peak_kb),
                    s.ctx
                ),
            ));
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::bordered()),
            detail,
        );
    }
}
