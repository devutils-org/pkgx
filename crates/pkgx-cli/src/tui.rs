//! Read-only by default; experimental APT/Pacman execution requires an opt-in and typed review.
mod theme;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use pkgx_core::{
    backend::Backend, manifest::DesiredState, InstallReason, InstalledPackage, PackageId, Plan,
    ProviderId,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
    Frame, Terminal,
};
use std::{
    cell::{Cell as StateCell, RefCell},
    io::{self, IsTerminal},
    sync::atomic::Ordering,
    time::Duration,
};

const HELP: &str = "NAVIGATION
Left / h: providers · Right / l: packages
j / k / arrows: move or scroll · Enter / i: package info
Tab / Shift-Tab: next / previous provider
Home / End: first / last · PgUp / PgDn: page

INSPECTION
/: filter installed packages · p: toggle cached preview
d: dependency expressions · w: installation reason · o: native orphan suggestions
a / r: install / removal preview · m: manifest preview
R: refresh inventory · Esc: back · q: quit

DATA
Summary counts follow the current provider and text filter.
Unknown reason means native installation intent is unavailable.
Cached preview reads the inventory; Enter queries native metadata.
DEMO means synthetic data, with no native planning or execution.

EXECUTION
x: review confirmation (requires --allow-apply and an APT or Pacman plan)
Type apply and press Enter to execute.

HELP
Esc / ?: close without losing the current review
j / k / PgUp / PgDn: scroll help · q: quit";

struct Pending {
    managed: Option<(std::path::PathBuf, pkgx_core::managed::ManagedState)>,
    plan: Plan,
    install: Vec<PackageId>,
    remove: Vec<PackageId>,
}
impl Pending {
    fn provider(&self) -> Option<ProviderId> {
        self.install
            .iter()
            .chain(&self.remove)
            .next()
            .map(|id| id.provider)
    }
}
#[derive(Default)]
struct Browser {
    packages: Vec<InstalledPackage>,
    query: String,
    providers: Vec<ProviderId>,
    provider: Option<ProviderId>,
    filtering: bool,
    sidebar_focus: bool,
    selected: usize,
    table_state: RefCell<TableState>,
    viewport_rows: StateCell<usize>,
    page_scroll_limit: StateCell<u16>,
    hide_preview: bool,
    help: bool,
    help_scroll: u16,
    help_limit: StateCell<u16>,
    page: Option<String>,
    scroll: u16,
    pending: Option<Pending>,
    demo: bool,
    allow_apply: bool,
    sudo_requested: bool,
    confirming: bool,
    confirmation: String,
}
impl Browser {
    fn visible(&self) -> Vec<&InstalledPackage> {
        pkgx_core::catalog::filter_installed(&self.packages, &self.query)
            .into_iter()
            .filter(|p| {
                self.provider
                    .is_none_or(|provider| p.id.provider == provider)
            })
            .collect()
    }
    fn cycle_provider(&mut self) {
        self.provider = match self.provider {
            None => self.providers.first().copied(),
            Some(current) => self
                .providers
                .iter()
                .position(|p| *p == current)
                .and_then(|index| self.providers.get(index + 1))
                .copied(),
        };
        self.selected = 0;
        self.page = None;
        self.scroll = 0;
        self.clear_review();
    }
    fn current(&self) -> Option<&InstalledPackage> {
        self.visible().get(self.selected).copied()
    }
    fn move_selection(&mut self, delta: isize) {
        let length = self.visible().len();
        self.selected = if length == 0 {
            0
        } else {
            self.selected.saturating_add_signed(delta).min(length - 1)
        };
    }
    fn navigate(&mut self, code: KeyCode) {
        let page = self.viewport_rows.get().max(1);
        let delta = match code {
            KeyCode::PageDown => page as isize,
            KeyCode::PageUp => -(page as isize),
            KeyCode::Home => isize::MIN,
            KeyCode::End => isize::MAX,
            _ => return,
        };
        if self.sidebar_focus {
            self.step_provider(delta);
        } else if self.page.is_some() {
            self.scroll = match code {
                KeyCode::Home => 0,
                KeyCode::End => self.page_scroll_limit.get(),
                _ => self
                    .scroll
                    .saturating_add_signed(delta.clamp(i16::MIN as isize, i16::MAX as isize) as i16)
                    .min(self.page_scroll_limit.get()),
            };
        } else {
            self.move_selection(delta);
        }
    }
    fn filter_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Enter | KeyCode::Esc => self.filtering = false,
            KeyCode::Backspace => {
                self.query.pop();
            }
            KeyCode::Char(ch) if !ch.is_control() => self.query.push(ch),
            _ => {}
        }
        self.selected = 0;
    }
    fn arm(&mut self) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        let Some(provider) = pending.provider() else {
            return false;
        };
        if !self.allow_apply
            || !pkgx_core::execution::executable(provider)
            || pending
                .plan
                .operations
                .iter()
                .any(|op| op.package.provider != provider)
            || pending.plan.operations.is_empty()
            || pending
                .install
                .iter()
                .chain(&pending.remove)
                .any(|id| id.provider != provider)
        {
            return false;
        }
        self.confirming = true;
        self.confirmation.clear();
        true
    }
    fn confirm_key(&mut self, code: KeyCode) -> bool {
        if !self.confirming {
            return false;
        }
        match code {
            KeyCode::Esc => {
                self.confirming = false;
                self.confirmation.clear();
            }
            KeyCode::Backspace => {
                self.confirmation.pop();
            }
            KeyCode::Char(ch) if !ch.is_control() && self.confirmation.len() < 32 => {
                self.confirmation.push(ch)
            }
            KeyCode::Enter if self.confirmation == "apply" => {
                self.confirming = false;
                self.confirmation.clear();
                return true;
            }
            _ => {}
        }
        false
    }
    fn clear_review(&mut self) {
        self.pending = None;
        self.confirming = false;
        self.confirmation.clear();
    }
    fn step_provider(&mut self, delta: isize) {
        let current = self
            .provider
            .and_then(|p| self.providers.iter().position(|v| *v == p))
            .map_or(0, |i| i + 1);
        let next = current
            .saturating_add_signed(delta)
            .min(self.providers.len());
        self.provider = next
            .checked_sub(1)
            .and_then(|i| self.providers.get(i))
            .copied();
        self.selected = 0;
        self.page = None;
        self.scroll = 0;
        self.clear_review();
    }
    fn draw(&self, frame: &mut Frame) {
        let [header, main, status, footer] = Layout::vertical([
            Constraint::Length(u16::from(frame.area().height >= 10)),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        self.draw_summary(frame, header);
        // Keep the package pane usable on narrow terminals.
        let content = if main.width >= 64 {
            let [sidebar, content] = Layout::horizontal([
                Constraint::Length((main.width / 4).clamp(20, 28)),
                Constraint::Min(1),
            ])
            .areas(main);
            self.draw_sidebar(frame, sidebar);
            content
        } else {
            main
        };
        self.draw_content(frame, content);
        let notice = if self.demo {
            super::demo::NOTICE
        } else if self.allow_apply && self.sudo_requested {
            "Experimental native execution · sudo after confirmation · x reviews the plan"
        } else if self.allow_apply {
            "Experimental native execution enabled · x review · type apply to execute"
        } else {
            "Read-only · native execution requires --allow-apply"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                notice,
                Style::default().fg(if self.demo || self.allow_apply {
                    theme::WARN
                } else {
                    theme::MUTED
                }),
            )),
            status,
        );
        self.draw_footer(frame, footer);
        if self.help {
            self.draw_help(frame);
        }
    }
    fn draw_summary(&self, frame: &mut Frame, area: Rect) {
        let packages = self.visible();
        let count = |reason| {
            packages
                .iter()
                .filter(|p| p.install_reason == reason)
                .count()
        };
        let source = if self.demo { " DEMO " } else { " CACHED " };
        let spans = vec![
            Span::styled(
                source,
                theme::badge(if self.demo {
                    theme::WARN
                } else {
                    theme::ACCENT
                }),
            ),
            Span::raw(format!(
                " {}/{} visible · ",
                packages.len(),
                self.packages.len()
            )),
            Span::styled(
                format!("{} explicit", count(InstallReason::Explicit)),
                theme::reason(InstallReason::Explicit),
            ),
            Span::raw(" · "),
            Span::styled(
                format!("{} dependency", count(InstallReason::Dependency)),
                theme::reason(InstallReason::Dependency),
            ),
            Span::raw(" · "),
            Span::styled(
                format!("{} unknown reason", count(InstallReason::Unknown)),
                theme::reason(InstallReason::Unknown),
            ),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
    fn open_help(&mut self) {
        self.help = true;
        self.help_scroll = 0;
    }
    fn help_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('?') => self.help = false,
            KeyCode::Down | KeyCode::Char('j') => {
                self.help_scroll = self
                    .help_scroll
                    .saturating_add(1)
                    .min(self.help_limit.get())
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.help_scroll = self.help_scroll.saturating_sub(1)
            }
            KeyCode::PageDown => {
                self.help_scroll = self
                    .help_scroll
                    .saturating_add(8)
                    .min(self.help_limit.get())
            }
            KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(8),
            KeyCode::Home => self.help_scroll = 0,
            KeyCode::End => self.help_scroll = self.help_limit.get(),
            _ => {}
        }
    }
    fn draw_help(&self, frame: &mut Frame) {
        let screen = frame.area();
        let width = screen
            .width
            .saturating_sub(4)
            .min(88)
            .max(screen.width.min(1));
        let height = screen
            .height
            .saturating_sub(2)
            .min(24)
            .max(screen.height.min(1));
        let area = Rect::new(
            screen.x + (screen.width - width) / 2,
            screen.y + (screen.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        let block = theme::block(" Keyboard help · Esc / ? close ".into(), true);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let paragraph = Paragraph::new(HELP).wrap(Wrap { trim: false });
        self.help_limit.set(
            paragraph
                .line_count(inner.width)
                .saturating_sub(inner.height as usize)
                .min(u16::MAX as usize) as u16,
        );
        frame.render_widget(
            paragraph.scroll((self.help_scroll.min(self.help_limit.get()), 0)),
            inner,
        );
    }
    fn draw_sidebar(&self, frame: &mut Frame, area: Rect) {
        let block = theme::block(" Providers ".into(), self.sidebar_focus);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let entries = std::iter::once(None).chain(self.providers.iter().copied().map(Some));
        let lines: Vec<_> = entries
            .map(|provider| {
                let count = self
                    .packages
                    .iter()
                    .filter(|p| provider.is_none_or(|id| p.id.provider == id))
                    .count();
                let name = provider.map_or_else(|| "All packages".into(), |p| p.to_string());
                let style = if self.provider == provider {
                    if self.sidebar_focus {
                        theme::badge(theme::ACCENT)
                    } else {
                        Style::default()
                            .fg(theme::ACCENT)
                            .add_modifier(Modifier::BOLD)
                    }
                } else {
                    Style::default()
                };
                Line::styled(format!(" {name} ({count})"), style)
            })
            .collect();
        // Reveal the current provider even in short terminals.
        let index = self
            .provider
            .and_then(|p| self.providers.iter().position(|v| *v == p))
            .map_or(0, |i| i + 1);
        let scroll = (index + 1).saturating_sub(inner.height as usize) as u16;
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
    }
    fn draw_content(&self, frame: &mut Frame, area: Rect) {
        let title = if self.confirming {
            " Confirm native execution ".into()
        } else if self.filtering {
            " Search packages ".into()
        } else if self.page.is_some() {
            " Package / reviewed plan ".into()
        } else {
            format!(
                " pkgx{} · {} packages ",
                if self.demo { " DEMO" } else { "" },
                self.visible().len()
            )
        };
        let block = theme::block(
            title,
            !self.sidebar_focus || self.filtering || self.confirming,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [input, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
        let line = if self.confirming {
            Line::from(vec![
                Span::styled("Type apply: ", Style::default().fg(theme::WARN)),
                Span::raw(format!("{}█", self.confirmation)),
            ])
        } else {
            Line::from(vec![
                Span::styled("/ ", Style::default().fg(theme::SEARCH)),
                Span::raw(self.query.clone()),
                Span::styled(
                    if self.filtering { "█" } else { "" },
                    Style::default().fg(theme::MUTED),
                ),
            ])
        };
        frame.render_widget(Paragraph::new(line), input);
        self.viewport_rows
            .set(body.height.saturating_sub(1) as usize);
        if let Some(page) = &self.page {
            let paragraph = Paragraph::new(page.as_str()).wrap(Wrap { trim: false });
            self.page_scroll_limit.set(
                paragraph
                    .line_count(body.width)
                    .saturating_sub(body.height as usize)
                    .min(u16::MAX as usize) as u16,
            );
            frame.render_widget(
                paragraph.scroll((self.scroll.min(self.page_scroll_limit.get()), 0)),
                body,
            );
        } else {
            self.viewport_rows
                .set(body.height.saturating_sub(1) as usize);
            let body = if body.width >= 100 && !self.hide_preview {
                let [list, preview] =
                    Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
                        .areas(body);
                self.draw_snapshot(frame, preview);
                list
            } else {
                body
            };
            let visible = self.visible();
            if visible.is_empty() {
                frame.render_widget(
                    Paragraph::new("No packages match. Tab changes provider; / edits search.")
                        .style(Style::default().fg(theme::MUTED)),
                    body,
                );
                return;
            }
            let rows = visible.into_iter().map(|p| {
                Row::new(vec![
                    Cell::from(p.id.to_string()),
                    Cell::from(p.version.clone()),
                    Cell::from(format!("{:?}", p.install_reason))
                        .style(theme::reason(p.install_reason)),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Percentage(55),
                    Constraint::Percentage(25),
                    Constraint::Percentage(20),
                ],
            )
            .header(
                Row::new(["Package / scope", "Version", "Reason"])
                    .style(Style::default().fg(theme::MUTED)),
            )
            .row_highlight_style(if self.sidebar_focus {
                Style::default().fg(theme::ACCENT)
            } else {
                theme::badge(theme::ACCENT)
            })
            .highlight_symbol("› ");
            let mut selection = self.table_state.borrow_mut();
            selection.select(Some(self.selected));
            frame.render_stateful_widget(table, body, &mut *selection);
        }
    }
    fn draw_snapshot(&self, frame: &mut Frame, area: Rect) {
        let block = theme::block(" Installed snapshot ".into(), false);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let Some(package) = self.current() else {
            return;
        };
        let mut lines = vec![
            Line::styled(
                package.id.to_string(),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::raw(format!("Version  {}", package.version)),
            Line::styled(
                format!("Reason   {:?}", package.install_reason),
                theme::reason(package.install_reason),
            ),
            Line::raw(""),
        ];
        // This preview uses only the cached inventory; moving never runs native commands.
        for (key, value) in &package.metadata {
            let clean = value
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>();
            lines.push(Line::styled(key.clone(), Style::default().fg(theme::MUTED)));
            lines.push(Line::raw(clean));
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(
            "Enter: query full package info · p: hide preview",
            Style::default().fg(theme::MUTED),
        ));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }
    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let (mode, color, keys): (&str, _, &[(&str, &str)]) = if self.confirming {
            (
                "CONFIRM",
                theme::WARN,
                &[("Esc", "cancel"), ("Enter", "confirm typed apply")],
            )
        } else if self.filtering {
            (
                "SEARCH",
                theme::SEARCH,
                &[("Esc", "close"), ("Enter", "done"), ("Backspace", "erase")],
            )
        } else if self.sidebar_focus {
            (
                "PROVIDERS",
                theme::ACCENT,
                &[
                    ("q", "quit"),
                    ("jk", "provider"),
                    ("Enter", "packages"),
                    ("Tab", "next"),
                    ("?", "help"),
                ],
            )
        } else if self.page.is_some() && self.pending.is_some() && self.allow_apply {
            (
                "REVIEW",
                theme::ACCENT,
                &[
                    ("q", "quit"),
                    ("Esc", "back"),
                    ("jk", "scroll"),
                    ("x", "execute"),
                    ("Tab", "provider"),
                ],
            )
        } else if self.page.is_some() {
            (
                "DETAILS",
                theme::ACCENT,
                &[
                    ("q", "quit"),
                    ("Esc", "back"),
                    ("jk", "scroll"),
                    ("PgUp/Dn", "page"),
                    ("?", "help"),
                ],
            )
        } else {
            (
                "NORMAL",
                theme::ACCENT,
                &[
                    ("q", "quit"),
                    ("?", "help"),
                    ("jk", "move"),
                    ("Enter", "info"),
                    ("/", "search"),
                    ("Tab", "provider"),
                    ("←→", "pane"),
                    ("p", "preview"),
                    ("d", "deps"),
                    ("w", "why"),
                    ("a/r", "preview"),
                    ("o", "orphans"),
                    ("m", "manifest"),
                    ("R", "refresh"),
                ],
            )
        };
        let mut spans = vec![
            Span::styled(format!(" {mode} "), theme::badge(color)),
            Span::raw(" "),
        ];
        let mut used = mode.len() + 3;
        for (key, label) in keys {
            let width = key.chars().count() + label.chars().count() + 3;
            if used + width > area.width as usize {
                break;
            }
            used += width;
            spans.push(Span::styled(
                *key,
                Style::default().add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {label}  "),
                Style::default().fg(theme::MUTED),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
    fn set_page(&mut self, result: Result<String, Box<dyn std::error::Error>>) {
        self.page = Some(result.unwrap_or_else(|e| format!("Error: {e}")));
        self.page_scroll_limit.set(0);
        self.scroll = 0;
    }
}
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}
pub fn run(
    backends: &[Box<dyn Backend + '_>],
    runner: &super::session::Session,
    manifest: Option<&DesiredState>,
    allow_apply: bool,
    state_dir: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("tui requires interactive stdin and stdout".into());
    }
    let packages = read_inventory(backends)?;
    let canceled = &runner.canceled;
    let mut browser = Browser {
        demo: packages.iter().any(|p| {
            p.metadata
                .get("description")
                .is_some_and(|d| d == super::demo::NOTICE)
        }),
        packages,
        providers: backends.iter().map(|b| b.provider()).collect(),
        allow_apply,
        sudo_requested: runner.sudo_requested,
        ..Default::default()
    };
    enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    while !canceled.load(Ordering::Relaxed) {
        terminal.draw(|frame| browser.draw(frame))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(event::KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            break;
        }
        if browser.help {
            if key.code == KeyCode::Char('q') {
                break;
            }
            browser.help_key(key.code);
            continue;
        }
        if browser.confirming {
            if browser.confirm_key(key.code) {
                let pending = browser.pending.take().expect("armed review");
                apply_pending(
                    &mut terminal,
                    &mut browser,
                    runner,
                    pending,
                    state_dir.clone(),
                )?;
                if !canceled.load(Ordering::Relaxed) {
                    match read_inventory(backends) {
                        Ok(packages) => {
                            browser.packages = packages;
                            browser.selected = 0;
                        }
                        Err(error) => {
                            browser
                                .page
                                .as_mut()
                                .expect("execution result page")
                                .push_str(&format!(
                                "\nInventory refresh failed: {error}; browser snapshot is stale."
                            ));
                        }
                    }
                }
            }
            continue;
        }
        if browser.filtering {
            browser.filter_key(key.code);
            continue;
        }
        match key.code {
            KeyCode::Char('q') => break,
            KeyCode::Char('?') => browser.open_help(),
            KeyCode::Left | KeyCode::Char('h') => browser.sidebar_focus = true,
            KeyCode::Right | KeyCode::Char('l') => browser.sidebar_focus = false,
            KeyCode::Down | KeyCode::Char('j') if browser.sidebar_focus => browser.step_provider(1),
            KeyCode::Up | KeyCode::Char('k') if browser.sidebar_focus => browser.step_provider(-1),
            KeyCode::Enter if browser.sidebar_focus => browser.sidebar_focus = false,
            KeyCode::Tab => browser.cycle_provider(),
            KeyCode::BackTab => {
                if browser.provider.is_none() {
                    browser.step_provider(isize::MAX);
                } else {
                    browser.step_provider(-1);
                }
            }
            KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown => {
                browser.navigate(key.code)
            }
            KeyCode::Char('p') => browser.hide_preview = !browser.hide_preview,
            KeyCode::Esc => {
                browser.page = None;
                browser.scroll = 0;
                browser.clear_review();
            }
            KeyCode::Down | KeyCode::Char('j') if browser.page.is_some() => {
                browser.scroll = browser
                    .scroll
                    .saturating_add(1)
                    .min(browser.page_scroll_limit.get())
            }
            KeyCode::Up | KeyCode::Char('k') if browser.page.is_some() => {
                browser.scroll = browser.scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => browser.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => browser.move_selection(-1),
            KeyCode::Char('/') => {
                browser.page = None;
                browser.clear_review();
                browser.sidebar_focus = false;
                browser.filtering = true;
            }
            KeyCode::Char('x') => {
                if !browser.arm() {
                    browser.set_page(Ok("Execution is unavailable: enable --allow-apply and review a nonempty APT or Pacman plan first. Other providers have no executor.".into()));
                    browser.clear_review();
                }
            }
            KeyCode::Char('R') => {
                browser.clear_review();
                match read_inventory(backends) {
                    Ok(packages) => {
                        browser.packages = packages;
                        browser.selected = 0;
                        browser.page = None;
                    }
                    Err(error) => browser.set_page(Err(error)),
                }
            }
            KeyCode::Char('m') => {
                browser.clear_review();
                let result = (|| {
                    let desired = manifest.ok_or("provide tui --file to inspect a manifest")?;
                    let references: Vec<&dyn Backend> =
                        backends.iter().map(|b| b.as_ref()).collect();
                    let managed = if desired.system.mode == pkgx_core::manifest::SyncMode::Strict {
                        let path = super::state_directory(state_dir.clone())?.join("managed");
                        let snapshot = pkgx_core::managed::ManagedRegistry::new(&path).read()?;
                        Some((path, snapshot))
                    } else {
                        None
                    };
                    let review = if let Some((_, snapshot)) = &managed {
                        pkgx_core::planner::plan_manifest_multi_managed(
                            &references,
                            desired,
                            snapshot,
                        )?
                    } else {
                        pkgx_core::planner::plan_manifest_multi(&references, desired)?
                    };
                    let actionable: Vec<_> = review
                        .providers
                        .iter()
                        .filter(|p| !p.plan.operations.is_empty())
                        .collect();
                    // Strict grants exist for APT only; additive plans may target
                    // any provider with an executor.
                    if actionable.len() == 1
                        && pkgx_core::execution::executable(actionable[0].provider)
                        && (managed.is_none() || actionable[0].provider == ProviderId::Apt)
                    {
                        let provider = actionable[0].provider;
                        browser.pending = Some(Pending {
                            managed,
                            plan: actionable[0].plan.clone(),
                            install: review
                                .diff
                                .missing
                                .iter()
                                .filter(|id| id.provider == provider)
                                .cloned()
                                .collect(),
                            remove: review.diff.removals.clone(),
                        });
                    }
                    super::output::human(&review).map_err(Into::into)
                })();
                browser.set_page(result);
            }
            code @ (KeyCode::Enter | KeyCode::Char('i' | 'd' | 'w' | 'o' | 'r' | 'a')) => {
                let action = if let KeyCode::Char(action) = code {
                    action
                } else {
                    'i'
                };
                browser.clear_review();
                let Some(id) = browser.current().map(|p| p.id.clone()) else {
                    continue;
                };
                let backend = backends
                    .iter()
                    .find(|b| b.provider() == id.provider)
                    .expect("inventory backend");
                browser.page = Some("Reading native metadata…".into());
                terminal.draw(|frame| browser.draw(frame))?;
                let result = (|| {
                    Ok(match action {
                        'i' => super::output::human(&backend.info(&id)?)?,
                        'd' => super::output::human(&backend.dependencies(&id)?)?,
                        'w' => super::output::human(&backend.why(&id)?)?,
                        'o' => super::output::human(&pkgx_core::audit::inspect(backend.as_ref())?)?,
                        'r' | 'a' => {
                            let (install, remove) = if action == 'a' {
                                (vec![id], vec![])
                            } else {
                                (vec![], vec![id])
                            };
                            let plan = backend.plan(&install, &remove)?;
                            let text = super::output::human(&plan)?;
                            browser.pending = Some(Pending {
                                managed: None,
                                plan,
                                install,
                                remove,
                            });
                            text
                        }
                        _ => unreachable!(),
                    })
                })();
                browser.set_page(result);
            }
            _ => {}
        }
    }
    if canceled.load(Ordering::Relaxed) {
        return Err(
            pkgx_core::Error::Canceled("TUI session; inspect transaction history".into()).into(),
        );
    }
    Ok(())
}
fn read_inventory(
    backends: &[Box<dyn Backend + '_>],
) -> Result<Vec<InstalledPackage>, Box<dyn std::error::Error>> {
    let mut packages = vec![];
    for backend in backends {
        packages.extend(backend.inventory()?);
    }
    packages.sort_by_key(|p| (p.id.to_string(), p.version.clone()));
    Ok(packages)
}
struct CancelWorkerOnError<'a> {
    canceled: &'a std::sync::atomic::AtomicBool,
    completed: bool,
}
impl Drop for CancelWorkerOnError<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.canceled.store(true, Ordering::Relaxed);
        }
    }
}
fn apply_pending(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    browser: &mut Browser,
    runner: &super::session::Session,
    pending: Pending,
    state_dir: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = super::state_directory(state_dir)?;
    let use_sudo = if runner.sudo_requested && !pending.plan.operations.is_empty() {
        // sudo owns password entry in the normal terminal. Always restore the TUI,
        // including on authentication refusal, before displaying the result.
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show)?;
        let authorization = pkgx_core::privilege::authorize_apt(
            &pkgx_core::privilege::ProcessSudoAuthenticator {
                provider: pending.provider().unwrap_or(ProviderId::Apt),
            },
            true,
            &pending.plan,
            true,
            &runner.canceled,
        );
        execute!(io::stdout(), EnterAlternateScreen)?;
        enable_raw_mode()?;
        terminal.clear()?;
        match authorization {
            Ok(authorized) => authorized,
            Err(error) => {
                browser.clear_review();
                browser.set_page(Err(error.into()));
                return Ok(());
            }
        }
    } else {
        false
    };
    let started = std::time::Instant::now();
    let record = std::thread::scope(|scope| -> Result<_, Box<dyn std::error::Error>> {
        let worker = scope.spawn(|| {
            let journal = pkgx_core::journal::Journal::new(directory);
            let elevated = pkgx_core::privilege::SudoAptRunner::new(runner);
            let execution_runner: &dyn pkgx_core::command::CommandRunner =
                if use_sudo { &elevated } else { runner };
            if let Some((path, snapshot)) = &pending.managed {
                let registry = pkgx_core::managed::ManagedRegistry::new(path);
                let guard = registry.lock(snapshot)?;
                pkgx_core::execution::apply_managed_apt(
                    execution_runner,
                    &pending.plan,
                    &pending.install,
                    &pending.remove,
                    true,
                    &runner.canceled,
                    Some(&journal),
                    &guard,
                )
            } else {
                pkgx_core::execution::apply_recorded(
                    execution_runner,
                    &pending.plan,
                    &pending.install,
                    &pending.remove,
                    true,
                    &runner.canceled,
                    Some(&journal),
                )
            }
        });
        // On a terminal/event failure, request cancellation before the scoped
        // thread is joined; otherwise unwinding could wait for a long transaction.
        let mut cancel_on_error = CancelWorkerOnError {
            canceled: &runner.canceled,
            completed: false,
        };
        while !worker.is_finished() {
            browser.page=Some(format!("Applying reviewed native transaction… {} seconds\nCtrl-C cancels; a canceled native transaction can leave partial changes.\n",started.elapsed().as_secs()));
            terminal.draw(|frame| browser.draw(frame))?;
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press
                        && key.modifiers.contains(event::KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        runner.canceled.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
        cancel_on_error.completed = true;
        worker
            .join()
            .map_err(|_| "transaction worker interrupted; inspect history")?
            .map_err(Into::into)
    });
    browser.clear_review();
    browser.set_page(record.and_then(|record| super::output::human(&record).map_err(Into::into)));
    terminal.draw(|frame| browser.draw(frame))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    #[test]
    fn filtering_and_small_terminal_render_without_native_calls() {
        let mut browser = Browser {
            packages: pkgx_core::apt::parse_inventory(include_str!(
                "../../../tests/fixtures/dpkg.tsv"
            ))
            .unwrap(),
            ..Default::default()
        };
        browser.filter_key(KeyCode::Char('c'));
        browser.filter_key(KeyCode::Char('u'));
        assert_eq!(browser.visible().len(), 1);
        browser.move_selection(100);
        assert_eq!(browser.selected, 0);
        assert_eq!(
            browser.current().unwrap().id.provider,
            pkgx_core::ProviderId::Apt
        );
        let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
        terminal.draw(|frame| browser.draw(frame)).unwrap();
        browser.page = Some("A native failure remains visible.".into());
        terminal.draw(|frame| browser.draw(frame)).unwrap();
        let mut tiny = Terminal::new(TestBackend::new(1, 1)).unwrap();
        tiny.draw(|frame| browser.draw(frame)).unwrap();
    }
    #[test]
    fn provider_cycle_preserves_query_and_invalidates_review() {
        let mut browser = Browser {
            packages: pkgx_core::apt::parse_inventory(include_str!(
                "../../../tests/fixtures/dpkg.tsv"
            ))
            .unwrap(),
            providers: vec![ProviderId::Apt, ProviderId::Brew],
            query: "cu".into(),
            selected: 99,
            page: Some("old plan".into()),
            pending: Some(pending(ProviderId::Apt)),
            allow_apply: true,
            ..Default::default()
        };
        assert!(browser.arm());
        browser.cycle_provider();
        assert_eq!(browser.provider, Some(ProviderId::Apt));
        assert_eq!(browser.visible().len(), 1);
        assert_eq!(browser.query, "cu");
        assert_eq!(browser.selected, 0);
        assert!(browser.pending.is_none());
        assert!(browser.page.is_none());
        assert!(!browser.confirming);
        browser.cycle_provider();
        assert_eq!(browser.provider, Some(ProviderId::Brew));
        assert!(browser.current().is_none());
        browser.move_selection(1);
        assert_eq!(browser.selected, 0);
        browser.cycle_provider();
        assert_eq!(browser.provider, None);
        assert_eq!(browser.visible().len(), 1);
        let mut empty = Browser::default();
        empty.cycle_provider();
        assert!(empty.current().is_none());
    }
    #[test]
    fn provider_pane_moves_without_wrapping_or_retaining_a_plan() {
        let mut browser = Browser {
            providers: vec![ProviderId::Apt, ProviderId::Brew],
            query: "curl".into(),
            pending: Some(pending(ProviderId::Apt)),
            page: Some("review".into()),
            selected: 4,
            ..Default::default()
        };
        browser.step_provider(-1);
        assert_eq!(browser.provider, None);
        browser.step_provider(1);
        assert_eq!(browser.provider, Some(ProviderId::Apt));
        browser.step_provider(99);
        assert_eq!(browser.provider, Some(ProviderId::Brew));
        browser.step_provider(1);
        assert_eq!(browser.provider, Some(ProviderId::Brew));
        assert_eq!(browser.query, "curl");
        assert_eq!(browser.selected, 0);
        assert!(browser.page.is_none());
        assert!(browser.pending.is_none());
    }
    #[test]
    fn two_pane_layout_and_modes_render_with_terminal_palette() {
        let mut browser = Browser {
            packages: read_inventory(&super::super::demo::backends()).unwrap(),
            providers: vec![ProviderId::Apt, ProviderId::Brew, ProviderId::Flatpak],
            demo: true,
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| browser.draw(f)).unwrap();
        let text = |terminal: &Terminal<TestBackend>| -> String {
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect()
        };
        let screen = text(&terminal);
        assert!(screen.contains("Providers"));
        assert!(screen.contains("All packages (5)"));
        assert!(screen.contains("DEMO"));
        assert!(screen.contains("NORMAL"));
        assert!(screen.contains("╭"));
        browser.filtering = true;
        browser.query = "absent".into();
        terminal.draw(|f| browser.draw(f)).unwrap();
        let screen = text(&terminal);
        assert!(screen.contains("SEARCH"));
        assert!(screen.contains("No packages match"));
        browser.filtering = false;
        browser.confirming = true;
        browser.confirmation = "app".into();
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert!(text(&terminal).contains("Type apply: app"));
        assert!(text(&terminal).contains("CONFIRM"));
        for (width, height) in [(1, 1), (30, 8), (64, 4)] {
            let mut small = Terminal::new(TestBackend::new(width, height)).unwrap();
            small.draw(|f| browser.draw(f)).unwrap();
        }
    }
    #[test]
    fn paging_keeps_table_viewport_stable_between_draws() {
        let template = super::super::demo::DemoBackend(ProviderId::Apt)
            .inventory()
            .unwrap()
            .remove(0);
        let mut browser = Browser {
            packages: (0..80)
                .map(|i| {
                    let mut package = template.clone();
                    package.id.name = format!("package{i:03}");
                    package
                })
                .collect(),
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal.draw(|f| browser.draw(f)).unwrap();
        let rows = browser.viewport_rows.get();
        assert!(rows > 0 && rows < 12);
        browser.navigate(KeyCode::PageDown);
        assert_eq!(browser.selected, rows);
        browser.navigate(KeyCode::End);
        assert_eq!(browser.selected, 79);
        terminal.draw(|f| browser.draw(f)).unwrap();
        let offset = browser.table_state.borrow().offset();
        assert!(offset > 0);
        browser.move_selection(-1);
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert_eq!(browser.table_state.borrow().offset(), offset);
        browser.navigate(KeyCode::Home);
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert_eq!(browser.table_state.borrow().offset(), 0);
        browser.navigate(KeyCode::PageUp);
        assert_eq!(browser.selected, 0);
    }
    #[test]
    fn cached_preview_follows_selection_and_can_be_hidden() {
        let mut browser = Browser {
            packages: read_inventory(&super::super::demo::backends()).unwrap(),
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(150, 25)).unwrap();
        terminal.draw(|f| browser.draw(f)).unwrap();
        let screen = |terminal: &Terminal<TestBackend>| -> String {
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect()
        };
        assert!(screen(&terminal).contains("Installed snapshot"));
        assert!(screen(&terminal).contains("Version  8.0-demo"));
        browser.navigate(KeyCode::End);
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert!(screen(&terminal).contains("Version  1.0-demo"));
        browser.hide_preview = true;
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert!(!screen(&terminal).contains("Installed snapshot"));
    }
    #[test]
    fn detail_paging_uses_wrapped_lines_and_stops_at_last_page() {
        let mut browser = Browser {
            page: Some("A long sentence with wide characters 漢字 and words. ".repeat(70)),
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(30, 10)).unwrap();
        terminal.draw(|f| browser.draw(f)).unwrap();
        let limit = browser.page_scroll_limit.get();
        assert!(limit > 20);
        browser.navigate(KeyCode::End);
        assert_eq!(browser.scroll, limit);
        browser.navigate(KeyCode::PageDown);
        assert_eq!(browser.scroll, limit);
        terminal.draw(|f| browser.draw(f)).unwrap();
        browser.navigate(KeyCode::PageUp);
        assert!(browser.scroll < limit);
        browser.navigate(KeyCode::Home);
        assert_eq!(browser.scroll, 0);
        browser.page = Some("Short detail".into());
        terminal.draw(|f| browser.draw(f)).unwrap();
        browser.navigate(KeyCode::End);
        assert_eq!(browser.scroll, 0);
    }
    #[test]
    fn summary_counts_follow_provider_and_filter() {
        let mut browser = Browser {
            packages: read_inventory(&super::super::demo::backends()).unwrap(),
            demo: true,
            ..Default::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        let header = |terminal: &Terminal<TestBackend>| -> String {
            terminal.backend().buffer().content[..120]
                .iter()
                .map(|c| c.symbol())
                .collect()
        };
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert!(header(&terminal).contains("5/5 visible"));
        assert!(header(&terminal).contains("3 explicit"));
        assert!(header(&terminal).contains("1 dependency"));
        assert!(header(&terminal).contains("1 unknown reason"));
        browser.provider = Some(ProviderId::Apt);
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert!(header(&terminal).contains("3/5 visible"));
        assert!(header(&terminal).contains("0 unknown reason"));
        browser.query = "absent".into();
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert!(header(&terminal).contains("0/5 visible"));
        assert!(header(&terminal).contains("0 explicit"));
    }
    #[test]
    fn help_overlay_preserves_review_and_consumes_action_keys() {
        let mut browser = Browser {
            page: Some("Reviewed native plan".into()),
            pending: Some(pending(ProviderId::Apt)),
            allow_apply: true,
            selected: 3,
            scroll: 2,
            query: "curl".into(),
            ..Default::default()
        };
        browser.open_help();
        let mut terminal = Terminal::new(TestBackend::new(80, 15)).unwrap();
        terminal.draw(|f| browser.draw(f)).unwrap();
        assert!(browser.help_limit.get() > 0);
        browser.help_key(KeyCode::Char('x'));
        browser.help_key(KeyCode::Enter);
        browser.help_key(KeyCode::Tab);
        assert!(!browser.confirming);
        browser.help_key(KeyCode::End);
        assert_eq!(browser.help_scroll, browser.help_limit.get());
        browser.help_key(KeyCode::Esc);
        assert!(!browser.help);
        assert_eq!(browser.page.as_deref(), Some("Reviewed native plan"));
        assert!(browser.pending.is_some());
        assert_eq!(browser.selected, 3);
        assert_eq!(browser.scroll, 2);
        assert_eq!(browser.query, "curl");
        assert!(browser.arm());
        browser.confirm_key(KeyCode::Esc);
        browser.open_help();
        for (width, height) in [(1, 1), (30, 8), (150, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| browser.draw(f)).unwrap();
        }
    }
    fn pending(provider: ProviderId) -> Pending {
        let id = pkgx_core::identity::parse_name(provider, "curl").unwrap();
        Pending {
            managed: None,
            plan: Plan {
                operations: vec![pkgx_core::Operation {
                    kind: pkgx_core::OperationKind::Install,
                    package: id.clone(),
                    risk: pkgx_core::RiskLevel::RequiresReview,
                    native_detail: "preview".into(),
                }],
                observed_state_fingerprint: String::new(),
                observed_packages: vec![],
                uncertain: true,
                warnings: vec![],
            },
            install: vec![id],
            remove: vec![],
        }
    }
    #[test]
    fn mutation_needs_opt_in_a_valid_review_and_the_exact_typed_confirmation() {
        let mut browser = Browser {
            pending: Some(pending(ProviderId::Apt)),
            ..Default::default()
        };
        assert!(!browser.arm());
        assert!(!browser.confirm_key(KeyCode::Enter));
        browser.allow_apply = true;
        assert!(browser.arm());
        assert!(!browser.confirm_key(KeyCode::Enter));
        for ch in "apply".chars() {
            assert!(!browser.confirm_key(KeyCode::Char(ch)));
        }
        assert!(browser.confirm_key(KeyCode::Enter));
        assert!(!browser.confirm_key(KeyCode::Enter));
        assert!(browser.arm());
        browser.confirm_key(KeyCode::Char('a'));
        assert!(!browser.confirm_key(KeyCode::Esc));
        assert!(!browser.confirm_key(KeyCode::Enter));
        browser.pending = Some(pending(ProviderId::Pacman));
        assert!(browser.arm());
        browser.confirm_key(KeyCode::Esc);
        let mut mixed = pending(ProviderId::Pacman);
        mixed.plan.operations[0].package.provider = ProviderId::Apt;
        browser.pending = Some(mixed);
        assert!(!browser.arm());
        let flatpak = pkgx_core::identity::parse_name(
            ProviderId::Flatpak,
            "app/org.example.App/x86_64/stable:user",
        )
        .unwrap();
        let mut unsupported = pending(ProviderId::Apt);
        unsupported.plan.operations[0].package = flatpak.clone();
        unsupported.install = vec![flatpak];
        browser.pending = Some(unsupported);
        assert!(!browser.arm());
        browser.pending = Some(pending(ProviderId::Apt));
        browser.pending.as_mut().unwrap().plan.operations.clear();
        assert!(!browser.arm());
    }
    #[test]
    fn terminal_error_cancels_a_scoped_worker_before_joining() {
        let canceled = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let start = std::time::Instant::now();
                while !canceled.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(1)
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                canceled.load(Ordering::Relaxed)
            });
            {
                let _cancel = CancelWorkerOnError {
                    canceled: &canceled,
                    completed: false,
                };
            }
            assert!(worker.join().unwrap());
        });
    }
}
