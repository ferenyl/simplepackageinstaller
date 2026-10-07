use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::config::{ChoiceSet, Config};
use crate::deps;
use crate::source;
use crate::state::InstalledState;

use super::centered;

pub enum Action {
    None,
    Quit,
    Start(Vec<deps::Step>),
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
        let mut view = Self {
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
        };
        view.normalize_choices(cfg);
        view
    }

    /// At most one alternative per choice; a required choice always has one.
    fn normalize_choices(&mut self, cfg: &Config) {
        for choice in &cfg.choices {
            let chosen: Vec<usize> = choice.packages.iter().copied().filter(|&p| self.selected[p]).collect();
            for &p in chosen.iter().skip(1) {
                self.selected[p] = false;
                self.forced[p] = false;
            }
            if chosen.is_empty() && choice.required {
                let pick = self.default_pick(choice);
                self.selected[pick] = true;
            }
        }
    }

    fn default_pick(&self, choice: &ChoiceSet) -> usize {
        choice.packages.iter().copied().find(|&p| self.installed[p]).unwrap_or(choice.packages[0])
    }

    /// Selects `p`, deselecting the other alternatives of its choice.
    fn choose(&mut self, cfg: &Config, p: usize) {
        if let Some(c) = cfg.packages[p].choice {
            for &other in &cfg.choices[c].packages {
                self.selected[other] = false;
                self.forced[other] = false;
            }
        }
        self.selected[p] = true;
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
            Row::Section(si) => cfg.sections[si].groups.iter().flat_map(|g| g.packages.iter().copied()).collect(),
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
                self.normalize_choices(cfg);
            }
            return Action::None;
        }

        let rows = self.rows(cfg);
        let cursor = self.list.selected().unwrap_or(0).min(rows.len().saturating_sub(1));
        let row = rows[cursor];
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::Up | KeyCode::Char('k') => self.list.select(Some(cursor.saturating_sub(1))),
            KeyCode::Down | KeyCode::Char('j') => self.list.select(Some((cursor + 1).min(rows.len() - 1))),
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
            KeyCode::Char('a') => {
                self.selected.fill(true);
                self.normalize_choices(cfg);
            }
            KeyCode::Char('n') => {
                self.selected.clone_from(&self.required);
                self.normalize_choices(cfg);
            }
            KeyCode::Char('u') => self.system_update = !self.system_update,
            KeyCode::Char('R') => {
                if let Row::Package(p) = row {
                    let forced = !self.forced[p];
                    if forced {
                        self.choose(cfg, p);
                    }
                    self.forced[p] = forced;
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
            Row::Group(si, gi) if self.expanded_groups[si][gi] => self.expanded_groups[si][gi] = false,
            Row::Group(..) | Row::Package(_) => {
                let parent = rows[..cursor].iter().rposition(|r| {
                    matches!((rows[cursor], r), (Row::Group(..), Row::Section(_)) | (Row::Package(_), Row::Group(..)))
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
                self.choose(cfg, p);
                return;
            }
            let name = &cfg.packages[p].name;
            if self.required[p] {
                self.notice = Some(format!("{name} is required and cannot be deselected"));
                return;
            }
            if cfg.packages[p].choice.is_some_and(|c| cfg.choices[c].required) {
                self.notice = Some(format!("{name} is a required choice; pick another alternative instead"));
                return;
            }
            let dependents = deps::required_by(cfg, &self.selected, &self.installed, p);
            let locked: Vec<&str> =
                dependents.iter().filter(|&&d| self.required[d]).map(|&d| cfg.packages[d].name.as_str()).collect();
            if !locked.is_empty() {
                self.notice = Some(format!("{name} is needed by required packages: {}", locked.join(", ")));
            } else if dependents.is_empty() {
                self.selected[p] = false;
                self.forced[p] = false;
            } else {
                self.confirm = Some(Confirm { target: p, dependents });
            }
            return;
        }
        let choice = match row {
            Row::Section(si) => cfg.sections[si].choice,
            Row::Group(si, gi) => cfg.sections[si].groups[gi].choice,
            Row::Package(_) => None,
        };
        if let Some(c) = choice {
            let choice = &cfg.choices[c];
            if !choice.packages.iter().any(|&p| self.selected[p]) {
                let pick = self.default_pick(choice);
                self.selected[pick] = true;
            } else if choice.required {
                self.notice = Some("A required choice; pick another alternative instead".into());
            } else {
                for &p in &choice.packages {
                    self.selected[p] = false;
                    self.forced[p] = false;
                }
            }
            return;
        }
        let pkgs: Vec<usize> = pkgs.into_iter().filter(|&p| !self.required[p]).collect();
        if pkgs.is_empty() {
            self.notice = Some("All packages here are required".into());
            return;
        }
        let all = pkgs.iter().all(|&p| self.selected[p]);
        for p in pkgs {
            self.selected[p] = !all;
            if all {
                self.forced[p] = false;
            }
        }
        self.normalize_choices(cfg);
    }

    pub fn render(&mut self, cfg: &Config, frame: &mut Frame) {
        let effective = self.effective(cfg);
        let rows = self.rows(cfg);
        let [list_area, info_area, help_area] =
            Layout::vertical([Constraint::Min(5), Constraint::Length(6), Constraint::Length(1)]).areas(frame.area());

        let items: Vec<ListItem> = rows.iter().map(|&r| self.row_item(cfg, r, &effective)).collect();
        let user = self.selected.iter().filter(|&&s| s).count();
        let auto = effective.iter().zip(&self.selected).filter(|&(&e, &s)| e && !s).count();
        let update = if self.system_update { "yes" } else { "no" };
        let title =
            format!(" simplepackageinstaller · {user} selected, +{auto} dependencies · system update: {update} ");
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
            Line::from(" ↑↓ move  ←→ fold  space select  a all  n none  u update  R rerun  enter install  q quit")
                .dim(),
            help_area,
        );

        if self.update_prompt {
            let text = vec![
                Line::from("Update the system (pacman -Syu) before installing?"),
                Line::from("Recommended, otherwise packages may be installed against outdated dependencies.").dim(),
                Line::from(""),
                Line::from("[y/enter] yes   [n] no   (change later with u)"),
            ];
            let area = centered(frame.area(), 64, 8);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: true }).block(Block::bordered().title(" system update ")),
                area,
            );
        }

        if let Some(confirm) = &self.confirm {
            let names: Vec<&str> = confirm.dependents.iter().map(|&d| cfg.packages[d].name.as_str()).collect();
            let text = vec![
                Line::from(format!("{} is needed by:", cfg.packages[confirm.target].name)),
                Line::from(names.join(", ")).bold(),
                Line::from(""),
                Line::from("[y] deselect them too   [n] cancel"),
            ];
            let area = centered(frame.area(), 60, 7);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: true }).block(Block::bordered().title(" required ")),
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
                let mut spans = vec![
                    Span::raw(format!("{arrow} {} ", self.check(&pkgs, effective))),
                    Span::styled(s.name.clone(), Style::new().bold().fg(Color::Cyan)),
                    Span::raw(self.count(cfg, &pkgs, effective)).dim(),
                ];
                if s.choice.is_some() {
                    spans.push(Span::raw("  one of").dim());
                }
                ListItem::new(Line::from(spans))
            }
            Row::Group(si, gi) => {
                let g = &cfg.sections[si].groups[gi];
                let arrow = if self.expanded_groups[si][gi] { "▾" } else { "▸" };
                let pkgs = Self::packages_of(cfg, row);
                let mut spans = vec![
                    Span::raw(format!("  {arrow} {} ", self.check(&pkgs, effective))),
                    Span::styled(g.name.clone(), Style::new().fg(Color::Yellow)),
                    Span::raw(self.count(cfg, &pkgs, effective)).dim(),
                ];
                if g.choice.is_some() {
                    spans.push(Span::raw("  one of").dim());
                }
                ListItem::new(Line::from(spans))
            }
            Row::Package(p) => {
                let pkg = &cfg.packages[p];
                let mark = match (self.selected[p], effective[p], pkg.choice.is_some()) {
                    (true, _, false) => "[x]",
                    (true, _, true) => "(•)",
                    (false, true, false) => "[+]",
                    (false, true, true) => "(+)",
                    (false, false, false) => "[ ]",
                    (false, false, true) => "( )",
                };
                let mut spans = vec![
                    Span::raw(format!("      {mark} ")),
                    Span::raw(pkg.name.clone()),
                    Span::styled(format!("  {}", pkg.source.tag()), Style::new().fg(Color::Blue)),
                ];
                if self.required[p] {
                    spans.push(Span::styled("  required", Style::new().fg(Color::Red)));
                }
                if self.forced[p] {
                    spans.push(Span::styled("  ↻ rerun", Style::new().fg(Color::Magenta)));
                }
                if self.installed[p] {
                    spans.push(Span::raw("  (installed)"));
                    return ListItem::new(Line::from(spans).dim());
                }
                ListItem::new(Line::from(spans))
            }
        }
    }

    /// "n of m", where the alternatives of a choice count as one.
    fn count(&self, cfg: &Config, pkgs: &[usize], effective: &[bool]) -> String {
        let mut units: Vec<Vec<usize>> = Vec::new();
        let mut seen_choices = Vec::new();
        for &p in pkgs {
            match cfg.packages[p].choice {
                Some(c) if seen_choices.contains(&c) => {}
                Some(c) => {
                    seen_choices.push(c);
                    units.push(cfg.choices[c].packages.clone());
                }
                None if units.iter().any(|u| u == &[p]) => {}
                None => units.push(vec![p]),
            }
        }
        let chosen = units.iter().filter(|u| u.iter().any(|&p| effective[p])).count();
        format!("  {chosen} of {}", units.len())
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
            return vec![Line::from(format!("{} packages, {installed} already installed", pkgs.len()))];
        };
        let pkg = &cfg.packages[p];
        let mut lines = vec![Line::from(vec![
            Span::raw(pkg.name.clone()).bold(),
            Span::raw(format!("  source: {}", pkg.source.tag())),
        ])];
        let mut field = |label: &str, values: &[String]| {
            if !values.is_empty() {
                lines.push(Line::from(format!("{label}: {}", values.join(", "))));
            }
        };
        field("flags", &pkg.flags);
        field("services", &pkg.service);
        field("user services", &pkg.user_service);
        field("post", &pkg.post);
        let mut requires: Vec<String> = pkg.requires.iter().map(|&r| cfg.packages[r].name.clone()).collect();
        requires.extend(pkg.requires_choice.iter().map(|&c| {
            let choice = &cfg.choices[c];
            choice.base.clone().unwrap_or_else(|| {
                let names: Vec<&str> = choice.packages.iter().map(|&m| cfg.packages[m].name.as_str()).collect();
                format!("one of {}", names.join("/"))
            })
        }));
        field("requires", &requires);
        if effective[p] && !self.selected[p] {
            let by: Vec<String> = deps::required_by(cfg, &self.selected, &self.installed, p)
                .into_iter()
                .map(|d| cfg.packages[d].name.clone())
                .collect();
            field("required by", &by);
        }
        if pkg.source == source::Source::Script {
            lines.push(Line::from(format!("script: {}", pkg.script_file())).dim());
        }
        lines
    }
}
