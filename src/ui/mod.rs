mod password;
mod run;
mod select;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::{DefaultTerminal, Frame};

use crate::config::Config;
use crate::runner::{self, CancelFlags, Context, Job};
use crate::source::{self, Source};
use crate::state::{self, InstalledState};

use run::RunView;
use select::{Action, SelectView};

enum Screen {
    Select(SelectView),
    Run(RunView),
}

pub struct App {
    cfg: Config,
    state: InstalledState,
    config_dir: PathBuf,
    dry_run: bool,
    screen: Screen,
}

impl App {
    pub fn new(cfg: Config, config_dir: PathBuf, dry_run: bool) -> Self {
        let state = InstalledState::load();
        let screen = Screen::Select(SelectView::new(&cfg, &state));
        Self { cfg, state, config_dir, dry_run, screen }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|f| self.render(f))?;
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
                match &mut self.screen {
                    Screen::Select(view) => match view.handle_key(&self.cfg, key) {
                        Action::Quit => return Ok(()),
                        _ if ctrl_c => return Ok(()),
                        Action::Start(order) => self.start(order),
                        Action::None => {}
                    },
                    Screen::Run(view) => {
                        if view.handle_key(key) {
                            return Ok(());
                        }
                    }
                }
            }
            if let Screen::Run(view) = &mut self.screen {
                view.tick();
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        match &mut self.screen {
            Screen::Select(view) => view.render(&self.cfg, frame),
            Screen::Run(view) => view.render(frame),
        }
    }

    fn start(&mut self, order: Vec<usize>) {
        let Screen::Select(view) = &self.screen else { return };
        let update = view.system_update;
        let offset = usize::from(update);
        let job_of = |p: usize| order.iter().position(|&o| o == p).map(|j| j + offset);
        let mut jobs: Vec<Job> = Vec::new();
        if update {
            jobs.push(Job {
                tag: "pacman",
                name: "System update".into(),
                deps: Vec::new(),
                skip: false,
                needs_sudo: true,
                install: "sudo pacman -Syu --noconfirm".into(),
                post: Vec::new(),
                marker: None,
            });
        }
        jobs.extend(order.iter().map(|&p| {
            let pkg = &self.cfg.packages[p];
            Job {
                tag: pkg.source.tag(),
                name: pkg.name.clone(),
                deps: self.cfg.requirements(p).filter_map(job_of).collect(),
                skip: view.installed[p] && !view.forced[p],
                needs_sudo: source::needs_sudo(pkg, &self.config_dir),
                install: source::install_command(pkg, &self.config_dir),
                post: source::post_commands(pkg),
                marker: (pkg.source == Source::Script && !pkg.always).then(|| self.state.marker(&pkg.name)),
            }
        }));

        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let cancel: CancelFlags = Arc::new(jobs.iter().map(|_| AtomicBool::new(false)).collect());
        let ctx = Context {
            cancel: cancel.clone(),
            config_dir: self.config_dir.clone(),
            dry_run: self.dry_run,
            log_file: state::state_dir().join("logs").join(format!("{stamp}.log")),
            shim_dir: state::runtime_dir().join("shim"),
        };
        let log_file = ctx.log_file.clone();
        let (tx, rx) = mpsc::channel();
        let (sudo_tx, sudo_rx) = mpsc::channel();
        runner::spawn(jobs.clone(), ctx, tx, sudo_rx);
        self.screen = Screen::Run(RunView::new(jobs, rx, sudo_tx, cancel, log_file));
    }
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}
