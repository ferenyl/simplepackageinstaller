use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use crate::runner::{CancelFlags, Event, Job, Status};

use super::password::{Outcome, PasswordPrompt};

pub struct RunView {
    jobs: Vec<Job>,
    status: Vec<Status>,
    reasons: Vec<Option<String>>,
    logs: Vec<Vec<String>>,
    list: ListState,
    follow: bool,
    fullscreen: bool,
    scroll: usize,
    done: bool,
    rx: Receiver<Event>,
    sudo_tx: Sender<bool>,
    cancel: CancelFlags,
    password: Option<PasswordPrompt>,
    log_file: PathBuf,
}

impl RunView {
    pub fn new(
        jobs: Vec<Job>,
        rx: Receiver<Event>,
        sudo_tx: Sender<bool>,
        cancel: CancelFlags,
        log_file: PathBuf,
    ) -> Self {
        let n = jobs.len();
        Self {
            jobs,
            status: vec![Status::Pending; n],
            reasons: vec![None; n],
            logs: vec![Vec::new(); n],
            list: ListState::default().with_selected(Some(0)),
            follow: true,
            fullscreen: false,
            scroll: 0,
            done: false,
            rx,
            sudo_tx,
            cancel,
            password: None,
            log_file,
        }
    }

    pub fn tick(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Started(i) => {
                    self.status[i] = Status::Running;
                    if self.follow {
                        self.list.select(Some(i));
                        self.scroll = 0;
                    }
                }
                Event::Line(i, text) => self.logs[i].push(text),
                Event::Finished(i, s, reason) => {
                    self.status[i] = s;
                    self.reasons[i] = reason;
                }
                Event::NeedSudo => self.password = Some(PasswordPrompt::default()),
                Event::AllDone => self.done = true,
            }
        }
    }

    /// Returns true when the user wants to quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let Some(prompt) = &mut self.password {
            match prompt.handle_key(key) {
                Outcome::Pending => {}
                Outcome::Accepted => {
                    self.password = None;
                    let _ = self.sudo_tx.send(true);
                }
                Outcome::Cancelled => {
                    self.password = None;
                    let _ = self.sudo_tx.send(false);
                }
            }
            return false;
        }
        let cursor = self.list.selected().unwrap_or(0);
        let last = self.jobs.len().saturating_sub(1);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc if self.done => return true,
            KeyCode::Up | KeyCode::Char('k') => self.select(cursor.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.select((cursor + 1).min(last)),
            KeyCode::Char('f') => {
                self.follow = true;
                if let Some(i) = self.status.iter().position(|&s| s == Status::Running) {
                    self.list.select(Some(i));
                }
            }
            KeyCode::Char('c') if !self.status[cursor].is_finished() => {
                self.cancel[cursor].store(true, Ordering::Relaxed);
            }
            KeyCode::Char('C') if !self.done => {
                for flag in self.cancel.iter() {
                    flag.store(true, Ordering::Relaxed);
                }
            }
            KeyCode::Enter => self.fullscreen = !self.fullscreen,
            KeyCode::PageUp => self.scroll += 10,
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            _ => {}
        }
        false
    }

    fn select(&mut self, i: usize) {
        self.follow = false;
        self.scroll = 0;
        self.list.select(Some(i));
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let [main, help] = Layout::vertical([Constraint::Min(5), Constraint::Length(1)]).areas(frame.area());
        let selected = self.list.selected().unwrap_or(0);

        if self.fullscreen {
            self.render_log(frame, main, selected);
        } else {
            let [list_area, log_area] =
                Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(main);
            let items: Vec<ListItem> = self
                .jobs
                .iter()
                .zip(&self.status)
                .enumerate()
                .map(|(i, (job, &s))| {
                    let (icon, color, label) = describe(s);
                    let label = match &self.reasons[i] {
                        Some(reason) => format!("{label}: {reason}"),
                        None if s == Status::Pending && self.cancel[i].load(Ordering::Relaxed) => {
                            "cancelling".to_string()
                        }
                        None => label.to_string(),
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" {icon} "), Style::new().fg(color)),
                        Span::raw(job.name.clone()),
                        Span::styled(format!("  {}", job.tag), Style::new().fg(Color::Blue)),
                        Span::styled(format!("  {label}"), Style::new().fg(color)),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(Block::bordered().title(self.title().bold()))
                .highlight_style(Style::new().bg(Color::DarkGray).add_modifier(Modifier::BOLD));
            frame.render_stateful_widget(list, list_area, &mut self.list);
            self.render_log(frame, log_area, selected);
        }

        let help_text = if self.done {
            format!(" done · log: {} · ↑↓ select  enter fullscreen  q quit", self.log_file.display())
        } else {
            " ↑↓ select  f follow current  enter fullscreen  PgUp/PgDn scroll  c cancel  C cancel all".to_string()
        };
        frame.render_widget(Line::from(help_text).dim(), help);

        if let Some(prompt) = &self.password {
            prompt.render(frame);
        }
    }

    fn render_log(&self, frame: &mut Frame, area: ratatui::layout::Rect, job: usize) {
        let log = &self.logs[job];
        let height = area.height.saturating_sub(2) as usize;
        let end = log.len().saturating_sub(self.scroll.min(log.len()));
        let start = end.saturating_sub(height);
        let lines: Vec<Line> = log[start..end].iter().map(|l| Line::from(l.as_str())).collect();
        let title = format!(" log · {} ", self.jobs[job].name);
        frame.render_widget(Paragraph::new(lines).block(Block::bordered().title(title)), area);
    }

    fn title(&self) -> String {
        let count = |f: fn(Status) -> bool| self.status.iter().filter(|&&s| f(s)).count();
        let finished = count(Status::is_finished);
        let failed = count(|s| s == Status::Failed);
        let warn = count(|s| s == Status::PostFailed);
        let skipped = count(|s| matches!(s, Status::Skipped | Status::Cancelled));
        format!(
            " {} {finished}/{} · ✗ {failed} · ⚠ {warn} · ⊘ {skipped} ",
            if self.done { "Done" } else { "Installing" },
            self.jobs.len()
        )
    }
}

fn describe(s: Status) -> (&'static str, Color, &'static str) {
    match s {
        Status::Pending => ("○", Color::Gray, "pending"),
        Status::Running => ("⟳", Color::Yellow, "running"),
        Status::Done => ("✓", Color::Green, "done"),
        Status::AlreadyInstalled => ("↷", Color::DarkGray, "already installed"),
        Status::PostFailed => ("⚠", Color::LightRed, "installed, post failed"),
        Status::Failed => ("✗", Color::Red, "failed"),
        Status::Skipped => ("⊘", Color::DarkGray, "skipped"),
        Status::Cancelled => ("⊘", Color::DarkGray, "cancelled"),
    }
}
