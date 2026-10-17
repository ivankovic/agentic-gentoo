//! Drawing: the searches along the top, the list or the thread, a footer that says what the keys
//! do here, and the `?` boxes.

use crate::app::{App, Mode, ThreadView, destination, mismatch};
use crate::render::{self, Kind, Target, when};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};

/// Every key, for `?` `?`. A section starts at column 0.
const HELP: &[&str] = &[
    "Everywhere",
    "  1-9, Tab Shift+Tab  a search along the top / the next, previous one",
    "  /                   search: notmuch's query language (from:ana subject:ručak date:1w..)",
    "  a  d                archive (leaves the Inbox) / delete (Gmail's Bin at the next sync)",
    "  s  m                star / unstar;  mark read / unread",
    "  u                   undo the last of these",
    "  ?                   explain what is selected; ? again: this screen",
    "  q  Esc              back; from the list, quit",
    "",
    "List",
    "  j k, arrows         move;  Ctrl+d Ctrl+u, PgDn PgUp  a page;  g G  top, bottom",
    "  Enter, l            read the thread",
    "",
    "Thread",
    "  j k  Space          scroll a line / a page;  Ctrl+d Ctrl+u  half a page;  g G",
    "  Tab  Shift+Tab      the next / previous link or attachment",
    "  Enter, o            open it: a link in the browser, an attachment with its program",
    "  y  w                copy the link / save the attachment to ~/Downloads",
    "  n  p                the next / previous message;  J K  the next / previous thread",
    "  H                   HTML or plain text, for mail that has both",
];

fn kind_style(k: Kind) -> Style {
    match k {
        Kind::Text => Style::new(),
        Kind::Quote | Kind::Sig | Kind::Meta | Kind::Rule => Style::new().fg(Color::DarkGray),
        Kind::From => Style::new().bold().fg(Color::Yellow),
        Kind::Strong => Style::new().bold(),
        Kind::Emph => Style::new().italic(),
        Kind::Code => Style::new().fg(Color::Green),
        Kind::Link => Style::new().fg(Color::Cyan).underlined(),
        Kind::Attachment => Style::new().fg(Color::Magenta).underlined(),
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let [r] = Layout::horizontal([Constraint::Length(w.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::vertical([Constraint::Length(h.min(area.height))])
        .flex(Flex::Center)
        .areas(r);
    r
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let [top, body, foot] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(f.area());

    // The searches, as tabs with their unread counts; a typed search after them.
    let mut tabs = vec![];
    for (i, s) in app.searches.iter().enumerate() {
        let on = app.current == Some(i);
        let mut text = format!(" {} {}", i + 1, s.name);
        if let Some(n) = app.unread[i].filter(|&n| n > 0) {
            text += &format!(" {n}");
        }
        text.push(' ');
        tabs.push(if on {
            Span::raw(text).bold().reversed()
        } else {
            Span::raw(text)
        });
    }
    if app.current.is_none() {
        tabs.push(Span::raw(format!(" / {} ", app.query)).bold().reversed());
    }
    f.render_widget(Line::from(tabs), top);

    if app.view.is_some() {
        draw_thread(f, app, body);
    } else {
        draw_list(f, app, body);
    }

    let footer = match &app.mode {
        Mode::Search(q) => Line::from(vec![Span::raw("/").bold(), Span::raw(q.clone()), Span::raw("▏")]),
        _ if !app.status.is_empty() => Line::from(app.status.clone()).yellow(),
        _ => match app.view.as_ref().and_then(|v| v.focus.map(|i| &v.doc.focus[i])) {
            Some(fp) => match &fp.target {
                Target::Link(url) => match mismatch(&fp.text, url) {
                    Some(warn) => Line::from(vec![Span::raw(format!("⚠ {warn} · ")).red().bold(), Span::raw(url.clone())]),
                    None => match destination(url) {
                        // A redirect: where it leads matters more than the tracker in front.
                        Some(dest) => Line::from(vec![
                            Span::raw("→ ").cyan(),
                            Span::raw(dest),
                            Span::raw(format!("  via {}", crate::app::host(url))).dark_gray(),
                            Span::raw("   Enter open · y copy").dark_gray(),
                        ]),
                        None => Line::from(vec![
                            Span::raw("→ ").cyan(),
                            Span::raw(url.clone()),
                            Span::raw("   Enter open · y copy").dark_gray(),
                        ]),
                    },
                },
                Target::Attachment { name, ctype, .. } => Line::from(vec![
                    Span::raw("📎 "),
                    Span::raw(format!("{name} ({ctype})")),
                    Span::raw("   Enter open · w save · y copy name").dark_gray(),
                ]),
            },
            None => Line::from(if app.view.is_some() {
                "? explain · Tab links · Enter open · n/p message · a archive · d delete · s star · q back"
            } else {
                "? explain · Enter read · / search · a archive · d delete · s star · m read/unread · u undo · q quit"
            })
            .dark_gray(),
        },
    };
    f.render_widget(footer, foot);

    match &app.mode {
        Mode::Explain { title, body, keys } => draw_explain(f, title, body, keys),
        Mode::Help => {
            let lines: Vec<Line> = HELP
                .iter()
                .map(|l| {
                    if l.starts_with(' ') || l.is_empty() {
                        Line::from(*l)
                    } else {
                        Line::from(*l).bold()
                    }
                })
                .collect();
            let area = centered(f.area(), 100, HELP.len() as u16 + 2);
            f.render_widget(Clear, area);
            f.render_widget(
                Paragraph::new(lines).block(Block::bordered().title(" Keys · Esc closes ")),
                area,
            );
        }
        _ => {}
    }
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = app.threads.iter().map(|t| {
        let unread = t.has("unread");
        let mark = if t.has("flagged") {
            Span::raw("★").yellow()
        } else if unread {
            Span::raw("●").cyan()
        } else {
            Span::raw(" ")
        };
        let mut subject = vec![Span::raw(t.subject.clone())];
        let extra: Vec<&str> = t
            .tags
            .iter()
            .map(String::as_str)
            .filter(|x| {
                !matches!(
                    *x,
                    "inbox"
                        | "unread"
                        | "flagged"
                        | "attachment"
                        | "replied"
                        | "sent"
                        | "signed"
                        | "encrypted"
                        | "passed"
                        | "draft"
                        | "archived"
                )
            })
            .collect();
        if !extra.is_empty() {
            subject.push(Span::raw(format!("  {}", extra.join(" "))).dark_gray());
        }
        let count = if t.total > 1 {
            format!("{}", t.total)
        } else {
            String::new()
        };
        let row = Row::new(vec![
            Cell::from(mark),
            Cell::from(render::name(
                t.authors.split([',', '|']).next().unwrap_or_default(),
            )),
            Cell::from(Span::raw(count).dark_gray()),
            Cell::from(Line::from(subject)),
            Cell::from(if t.has("attachment") { "📎" } else { "" }),
            Cell::from(Line::from(when(t.timestamp)).right_aligned()),
        ]);
        if unread { row.bold() } else { row }
    });
    let title = match app.current {
        Some(i) => format!(" {} ", app.searches[i].name),
        None => format!(" {} ", app.query),
    };
    let shown = format!(
        " {}{} threads ",
        app.threads.len(),
        if app.more { "+" } else { "" }
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Length(22),
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(2),
            Constraint::Length(10),
        ],
    )
    .block(
        Block::bordered()
            .title(title)
            .title_bottom(Line::from(shown).right_aligned()),
    )
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = TableState::default().with_selected(Some(app.sel));
    if app.threads.is_empty() {
        f.render_widget(
            Paragraph::new("Nothing here.")
                .dark_gray()
                .block(Block::bordered().title(" Empty ")),
            area,
        );
        return;
    }
    f.render_stateful_widget(table, area, &mut state);
}

fn draw_thread(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(v) = &mut app.view else { return };
    let block = Block::bordered().title(Line::from(format!(" {} ", v.thread.subject)).bold());
    let inner = block.inner(area).inner(Margin::new(1, 0));
    f.render_widget(block, area);
    v.layout(inner.width as usize);
    app.page = inner.height as usize;
    // Reading starts at the first unread message, or the last; when that leaves the screen half
    // empty, the messages before it fill it.
    if v.fresh {
        v.fresh = false;
        v.scroll = v.scroll.min(v.doc.lines.len().saturating_sub(app.page));
    }
    let lines: Vec<Line> = v
        .doc
        .lines
        .iter()
        .skip(v.scroll)
        .take(inner.height as usize)
        .map(|l| {
            Line::from(
                l.iter()
                    .map(|s| {
                        let mut st = kind_style(s.kind);
                        if s.focus.is_some() && s.focus == v.focus {
                            st = st.reversed();
                        }
                        Span::styled(s.text.clone(), st)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
    let pos = format!(" {}/{} ", v.current() + 1, v.msgs.len());
    let bottom = Rect {
        y: area.y + area.height - 1,
        ..area
    };
    f.render_widget(
        Line::from(pos).right_aligned().dark_gray(),
        bottom.inner(Margin::new(2, 0)),
    );
}

/// How many lines `text` takes wrapped at word boundaries to `width`, as `Wrap` lays it out.
fn wrapped_height(text: &str, width: usize) -> usize {
    use unicode_width::UnicodeWidthStr;
    let mut lines = 1;
    let mut used = 0;
    for word in text.split(' ') {
        let w = word.width();
        let need = if used == 0 { w } else { used + 1 + w };
        if need <= width {
            used = need;
        } else {
            lines += 1;
            // A word wider than the line is broken across lines.
            lines += w.saturating_sub(1) / width.max(1);
            used = w % width.max(1);
        }
    }
    lines
}

/// The `?` box: the explanation on the left, the keys that work on it on the right.
fn draw_explain(f: &mut Frame, title: &str, body: &[Line<'static>], keys: &[(&'static str, &'static str)]) {
    let width = f.area().width.saturating_sub(4).min(110);
    let text_width = width.saturating_sub(2 + 28 + 2).max(1) as usize;
    let wrapped: usize = body.iter().map(|l| wrapped_height(&l.to_string(), text_width)).sum();
    let height = (wrapped.max(keys.len() + 1) as u16 + 3).min(f.area().height.saturating_sub(2));
    let area = centered(f.area(), width, height);
    f.render_widget(Clear, area);
    let block = Block::bordered().title(format!(" {title} "));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let [main, foot] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
    let [text, side] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(28)]).areas(main);
    f.render_widget(
        Paragraph::new(body.to_vec()).wrap(Wrap { trim: false }),
        text.inner(Margin::new(1, 0)),
    );
    let mut lines = vec![Line::from("Keys here").bold()];
    for (k, what) in keys {
        lines.push(Line::from(vec![
            Span::raw(format!("{k:<6}")).bold(),
            Span::raw(*what),
        ]));
    }
    f.render_widget(Paragraph::new(lines), side);
    f.render_widget(
        Line::from(vec![
            Span::raw(" ?").bold(),
            Span::raw(" every key · "),
            Span::raw("Esc").bold(),
            Span::raw(" close"),
        ])
        .dark_gray(),
        foot,
    );
}

fn tags_line(tags: &[String]) -> Line<'static> {
    Line::from(vec![Span::raw("Tags: ").bold(), Span::raw(tags.join(" "))])
}

pub fn explain_list(app: &App) -> Mode {
    let keys = vec![
        ("Enter", "read it"),
        ("a", "archive"),
        ("d", "delete / restore"),
        ("s", "star"),
        ("m", "read / unread"),
        ("u", "undo"),
    ];
    let Some(t) = app.threads.get(app.sel) else {
        return Mode::Explain {
            title: "Nothing selected".into(),
            body: vec![Line::from("This search finds nothing.")],
            keys,
        };
    };
    let mut body = vec![
        Line::from(vec![
            Span::raw("From: ").bold(),
            Span::raw(t.authors.replace('|', ",")),
        ]),
        Line::from(format!(
            "{} message{}, {} of them matching this search; the newest {}.",
            t.total,
            if t.total == 1 { "" } else { "s" },
            t.matched,
            when(t.timestamp)
        )),
        tags_line(&t.tags),
        Line::from(""),
    ];
    body.push(Line::from(if t.has("inbox") {
        "a archives it: it leaves the Inbox and stays in All Mail. d deletes it: Gmail's Bin at the next sync, gone from there after 30 days."
    } else if t.has("deleted") {
        "It is in the Bin: d takes it back out. Gmail empties the Bin 30 days after deletion."
    } else {
        "Not in the Inbox. d deletes it: Gmail's Bin at the next sync, gone from there after 30 days."
    }));
    body.push(Line::from(
        "u undoes the last change, on exactly the messages it changed.",
    ));
    Mode::Explain {
        title: t.subject.clone(),
        body,
        keys,
    }
}

pub fn explain_thread(app: &App, v: &ThreadView) -> Mode {
    let _ = app;
    if let Some(fp) = v.focus.map(|i| &v.doc.focus[i]) {
        return match &fp.target {
            Target::Link(url) => {
                let mut body = vec![
                    Line::from(vec![Span::raw("Shown as: ").bold(), Span::raw(fp.text.clone())]),
                    Line::from(vec![Span::raw("Goes to: ").bold(), Span::raw(url.clone())]),
                    Line::from(vec![Span::raw("Site: ").bold(), Span::raw(crate::app::host(url))]),
                ];
                if let Some(dest) = destination(url) {
                    body.push(Line::from(vec![
                        Span::raw("Leads on to: ").bold(),
                        Span::raw(dest.clone()),
                    ]));
                    body.push(Line::from(format!(
                        "{} is a redirect, usually to count the click; it sends the browser on to {}.",
                        crate::app::host(url),
                        crate::app::host(&dest)
                    )));
                }
                if let Some(w) = mismatch(&fp.text, url) {
                    body.push(Line::from(""));
                    body.push(
                        Line::from(format!(
                            "⚠ {w}. Links like this are how phishing works; open it only if you expected it."
                        ))
                        .red(),
                    );
                }
                Mode::Explain {
                    title: "Link".into(),
                    body,
                    keys: vec![
                        ("Enter", "open in the browser"),
                        ("y", "copy it"),
                        ("Tab", "next link"),
                    ],
                }
            }
            Target::Attachment { name, ctype, .. } => Mode::Explain {
                title: "Attachment".into(),
                body: vec![
                    Line::from(vec![Span::raw("Name: ").bold(), Span::raw(name.clone())]),
                    Line::from(vec![Span::raw("Type: ").bold(), Span::raw(ctype.clone())]),
                    Line::from(""),
                    Line::from(
                        "Enter writes it to a private folder ($XDG_RUNTIME_DIR/mmm) and opens it with its program; w saves it to ~/Downloads.",
                    ),
                ],
                keys: vec![
                    ("Enter", "open"),
                    ("w", "save"),
                    ("y", "copy the name"),
                    ("Tab", "next"),
                ],
            },
        };
    }
    let m = &v.msgs[v.current()];
    let mut body = vec![
        Line::from(vec![Span::raw("From: ").bold(), Span::raw(m.from.clone())]),
        Line::from(vec![Span::raw("To: ").bold(), Span::raw(m.to.clone())]),
    ];
    if !m.cc.is_empty() {
        body.push(Line::from(vec![
            Span::raw("Cc: ").bold(),
            Span::raw(m.cc.clone()),
        ]));
    }
    body.push(Line::from(vec![
        Span::raw("Date: ").bold(),
        Span::raw(when(m.timestamp)),
    ]));
    body.push(tags_line(&m.tags));
    body.push(Line::from(""));
    body.push(Line::from(format!(
        "Message {} of {} in this thread; {} link{} and attachment{} to Tab through.",
        v.current() + 1,
        v.msgs.len(),
        v.doc.focus.len(),
        if v.doc.focus.len() == 1 { "" } else { "s" },
        if v.doc.focus.len() == 1 { "" } else { "s" },
    )));
    Mode::Explain {
        title: m.subject.clone(),
        body,
        keys: vec![
            ("Tab", "next link"),
            ("n p", "next / prev message"),
            ("J K", "next / prev thread"),
            ("H", "HTML / plain"),
            ("a d", "archive / delete"),
            ("s m", "star / unread"),
        ],
    }
}
