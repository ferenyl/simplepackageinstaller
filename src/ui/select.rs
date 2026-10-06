use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::config::Config;
use crate::deps;
use crate::source;
use crate::state::InstalledState;

use super::centered;

pub enum Action {
    None,
    Quit,
    Start(Vec<usize>),
}

#[derive(Clone, Copy)]
enum Row {
    Section(usize),
    Group(usize, usize),
    Package(usize),
}

struct Confirm {
    target: usize,
    dependents: Vec<usize>,
}

pub struct SelectView {
    pub installed: Vec<bool>,
    pub selected: Vec<bool>,
    pub forced: Vec<bool>,
    required: Vec<bool>,
    notice: Option<String>,
    expanded_sections: Vec<bool>,
    expanded_groups: Vec<Vec<bool>>,
    list: ListState,
    confirm: Option<Confirm>,
    update_prompt: bool,
    pub system_update: bool,
}

impl SelectView {
    pub fn new(cfg: &Config, state: &InstalledState) -> Self {
        let n = cfg.packages.len();
        Self {
            installed: cfg.packages.iter().map(|p| state.is_installed(p)).collect(),
            selected: cfg.packages.iter().map(|p| p.selected || p.required).collect(),
            forced: vec![false; n],
            required: cfg.packages.iter().map(|p| p.required).collect(),
            notice: None,
            expanded_sections: vec![false; cfg.sections.len()],
            expanded_groups: cfg.sections.iter().map(|s| vec![true; s.groups.len()]).collect(),
            list: ListState::default().with_selected(Some(0)),
            confirm: None,
            update_prompt: true,
            system_update: false,
        }
    }

    pub fn effective(&self, cfg: &Config) -> Vec<bool> {
        deps::closure(cfg, &self.selected, &self.installed)
    }

    fn rows(&self, cfg: &Config) -> Vec<Row> {
        let mut rows = Vec::new();
        for (si, s) in cfg.sections.iter().enumerate() {
            rows.push(Row::Section(si));
            if !self.expanded_sections[si] {
                continue;
            }
            for (gi, g) in s.groups.iter().enumerate() {
                rows.push(Row::Group(si, gi));
                if self.expanded_groups[si][gi] {
                    rows.extend(g.packages.iter().map(|&p| Row::Package(p)));
                }
            }
        }
        rows
    }

    fn packages_of(cfg: &Config, row: Row) -> Vec<usize> {
        match row {
            Row::Section(si) => cfg.sections[si]
                .groups
                .iter()
                .flat_map(|g| g.packages.iter().copied())
                .collect(),
            Row::Group(si, gi) => cfg.sections[si].groups[gi].packages.clone(),
            Row::Package(p) => vec![p],
        }
    }

    pub fn handle_key(&mut self, cfg: &Config, key: KeyEvent) -> Action {
        if self.update_prompt {
            self.update_prompt = false;
            self.system_update = matches!(key.code, KeyCode::Char('j' | 'y' | 'J' | 'Y') | KeyCode::Enter);
            return Action::None;
        }
        self.notice = None;
        if let Some(confirm) = self.confirm.take() {
            if matches!(key.code, KeyCode::Char('j' | 'y' | 'J' | 'Y')) {
                self.selected[confirm.target] = false;
                for d in confirm.dependents {
                    self.selected[d] = false;
                }
            }
            return Action::None;
        }

        let rows = self.rows(cfg);
        let cursor = self.list.selected().unwrap_or(0).min(rows.len().saturating_sub(1));
        let row = rows[cursor];
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => self.list.select(Some(cursor.saturating_sub(1))),
            KeyCode::Down | KeyCode::Char('j') => {
                self.list.select(Some((cursor + 1).min(rows.len() - 1)))
            }
            KeyCode::PageUp => self.list.select(Some(cursor.saturating_sub(15))),
            KeyCode::PageDown => self.list.select(Some((cursor + 15).min(rows.len() - 1))),
            KeyCode::Home | KeyCode::Char('g') => self.list.select(Some(0)),
            KeyCode::End | KeyCode::Char('G') => self.list.select(Some(rows.len() - 1)),
            KeyCode::Right | KeyCode::Char('l') => match row {
                Row::Section(si) => self.expanded_sections[si] = true,
                Row::Group(si, gi) => self.expanded_groups[si][gi] = true,
                Row::Package(_) => {}
            },
            KeyCode::Left | KeyCode::Char('h') => self.collapse(&rows, cursor),
            KeyCode::Char(' ') => self.toggle(cfg, row),
            KeyCode::Char('a') => self.selected.fill(true),
            KeyCode::Char('n') => self.selected.clone_from(&self.required),
            KeyCode::Char('u') => self.system_update = !self.system_update,
            KeyCode::Char('R') => {
                if let Row::Package(p) = row {
                    self.forced[p] = !self.forced[p];
                    if self.forced[p] {
                        self.selected[p] = true;
                    }
                }
            }
            KeyCode::Enter => {
                let effective = self.effective(cfg);
                if self.system_update || effective.iter().any(|&e| e) {
                    return Action::Start(deps::order(cfg, &effective));
                }
            }
            _ => {}
        }
        Action::None
    }

    fn collapse(&mut self, rows: &[Row], cursor: usize) {
        match rows[cursor] {
            Row::Section(si) => self.expanded_sections[si] = false,
            Row::Group(si, gi) if self.expanded_groups[si][gi] => {
                self.expanded_groups[si][gi] = false
            }
            Row::Group(..) | Row::Package(_) => {
                let parent = rows[..cursor].iter().rposition(|r| {
                    matches!(
                        (rows[cursor], r),
                        (Row::Group(..), Row::Section(_)) | (Row::Package(_), Row::Group(..))
                    )
                });
                if let Some(p) = parent {
                    self.list.select(Some(p));
                }
            }
        }
    }

    fn toggle(&mut self, cfg: &Config, row: Row) {
        let pkgs = Self::packages_of(cfg, row);
        if let Row::Package(p) = row {
            if !self.selected[p] {
                self.selected[p] = true;
                return;
            }
            let name = &cfg.packages[p].name;
            if self.required[p] {
                self.notice = Some(format!("{name} är obligatoriskt och kan inte kryssas ur"));
                return;
            }
            let dependents = deps::required_by(cfg, &self.selected, &self.installed, p);
            let locked: Vec<&str> = dependents
                .iter()
                .filter(|&&d| self.required[d])
                .map(|&d| cfg.packages[d].name.as_str())
                .collect();
            if !locked.is_empty() {
                self.notice = Some(format!("{name} krävs av obligatoriska paket: {}", locked.join(", ")));
            } else if dependents.is_empty() {
                self.selected[p] = false;
                self.forced[p] = false;
            } else {
                self.confirm = Some(Confirm { target: p, dependents });
            }
            return;
        }
        let pkgs: Vec<usize> = pkgs.into_iter().filter(|&p| !self.required[p]).collect();
        if pkgs.is_empty() {
            self.notice = Some("Alla paket här är obligatoriska".into());
            return;
        }
        let all = pkgs.iter().all(|&p| self.selected[p]);
        for p in pkgs {
            self.selected[p] = !all;
            if all {
                self.forced[p] = false;
            }
        }
    }

    pub fn render(&mut self, cfg: &Config, frame: &mut Frame) {
        let effective = self.effective(cfg);
        let rows = self.rows(cfg);
        let [list_area, info_area, help_area] = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(6),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let items: Vec<ListItem> = rows.iter().map(|&r| self.row_item(cfg, r, &effective)).collect();
        let user = self.selected.iter().filter(|&&s| s).count();
        let auto = effective.iter().zip(&self.selected).filter(|&(&e, &s)| e && !s).count();
        let update = if self.system_update { "ja" } else { "nej" };
        let title = format!(" simplearchpackageinstaller · {user} valda, +{auto} beroenden · systemuppdatering: {update} ");
        let list = List::new(items)
            .block(Block::bordered().title(title.bold()))
            .highlight_style(Style::new().bg(Color::DarkGray).add_modifier(Modifier::BOLD));
        frame.render_stateful_widget(list, list_area, &mut self.list);

        let cursor = self.list.selected().unwrap_or(0).min(rows.len().saturating_sub(1));
        let mut info = self.info(cfg, rows[cursor], &effective);
        if let Some(notice) = &self.notice {
            info.insert(0, Line::from(notice.clone()).fg(Color::Red));
        }
        frame.render_widget(
            Paragraph::new(info).wrap(Wrap { trim: true }).block(Block::bordered().title(" info ")),
            info_area,
        );
        frame.render_widget(
            Line::from(" ↑↓ flytta  ←→ fäll  space välj  a alla  n inga  u uppdatering  R kör om  enter installera  q avsluta")
                .dim(),
            help_area,
        );

        if self.update_prompt {
            let text = vec![
                Line::from("Uppdatera systemet (pacman -Syu) innan installationen?"),
                Line::from("Rekommenderas, annars kan paket installeras mot inaktuella beroenden.").dim(),
                Line::from(""),
                Line::from("[j/enter] ja   [n] nej   (ändra senare med u)"),
            ];
            let area = centered(frame.area(), 64, 8);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: true }).block(Block::bordered().title(" systemuppdatering ")),
                area,
            );
        }

        if let Some(confirm) = &self.confirm {
            let names: Vec<&str> = confirm.dependents.iter().map(|&d| cfg.packages[d].name.as_str()).collect();
            let text = vec![
                Line::from(format!("{} krävs av:", cfg.packages[confirm.target].name)),
                Line::from(names.join(", ")).bold(),
                Line::from(""),
                Line::from("[j] kryssa ur dem också   [n] avbryt"),
            ];
            let area = centered(frame.area(), 60, 7);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: true }).block(Block::bordered().title(" krävs ")),
                area,
            );
        }
    }

    fn row_item(&self, cfg: &Config, row: Row, effective: &[bool]) -> ListItem<'static> {
        match row {
            Row::Section(si) => {
                let s = &cfg.sections[si];
                let arrow = if self.expanded_sections[si] { "▾" } else { "▸" };
                let pkgs = Self::packages_of(cfg, row);
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{arrow} {} ", self.check(&pkgs, effective))),
                    Span::styled(s.name.clone(), Style::new().bold().fg(Color::Cyan)),
                ]))
            }
            Row::Group(si, gi) => {
                let g = &cfg.sections[si].groups[gi];
                let arrow = if self.expanded_groups[si][gi] { "▾" } else { "▸" };
                let pkgs = Self::packages_of(cfg, row);
                ListItem::new(Line::from(vec![
                    Span::raw(format!("  {arrow} {} ", self.check(&pkgs, effective))),
                    Span::styled(g.name.clone(), Style::new().fg(Color::Yellow)),
                ]))
            }
            Row::Package(p) => {
                let pkg = &cfg.packages[p];
                let mark = if self.selected[p] {
                    "[x]"
                } else if effective[p] {
                    "[+]"
                } else {
                    "[ ]"
                };
                let mut spans = vec![
                    Span::raw(format!("      {mark} ")),
                    Span::raw(pkg.name.clone()),
                    Span::styled(format!("  {}", pkg.source.tag()), Style::new().fg(Color::Blue)),
                ];
                if self.required[p] {
                    spans.push(Span::styled("  obligatorisk", Style::new().fg(Color::Red)));
                }
                if self.forced[p] {
                    spans.push(Span::styled("  ↻ kör om", Style::new().fg(Color::Magenta)));
                }
                if self.installed[p] {
                    spans.push(Span::raw("  (installerad)"));
                    return ListItem::new(Line::from(spans).dim());
                }
                ListItem::new(Line::from(spans))
            }
        }
    }

    fn check(&self, pkgs: &[usize], effective: &[bool]) -> &'static str {
        let selected = pkgs.iter().filter(|&&p| self.selected[p]).count();
        if selected == pkgs.len() && selected > 0 {
            "[x]"
        } else if selected > 0 {
            "[-]"
        } else if pkgs.iter().any(|&p| effective[p]) {
            "[+]"
        } else {
            "[ ]"
        }
    }

    fn info(&self, cfg: &Config, row: Row, effective: &[bool]) -> Vec<Line<'static>> {
        let Row::Package(p) = row else {
            let pkgs = Self::packages_of(cfg, row);
            let installed = pkgs.iter().filter(|&&p| self.installed[p]).count();
            return vec![Line::from(format!("{} paket, {installed} redan installerade", pkgs.len()))];
        };
        let pkg = &cfg.packages[p];
        let mut lines = vec![Line::from(vec![
            Span::raw(pkg.name.clone()).bold(),
            Span::raw(format!("  källa: {}", pkg.source.tag())),
        ])];
        let mut field = |label: &str, values: &[String]| {
            if !values.is_empty() {
                lines.push(Line::from(format!("{label}: {}", values.join(", "))));
            }
        };
        field("flaggor", &pkg.flags);
        field("tjänster", &pkg.service);
        field("user-tjänster", &pkg.user_service);
        field("post", &pkg.post);
        let requires: Vec<String> = pkg.requires.iter().map(|&r| cfg.packages[r].name.clone()).collect();
        field("kräver", &requires);
        if effective[p] && !self.selected[p] {
            let by: Vec<String> = deps::required_by(cfg, &self.selected, &self.installed, p)
                .into_iter()
                .map(|d| cfg.packages[d].name.clone())
                .collect();
            field("krävs av", &by);
        }
        if pkg.source == source::Source::Script {
            lines.push(Line::from(format!("script: {}", pkg.script_file())).dim());
        }
        lines
    }
}
