use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::sudo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Running,
    Done,
    AlreadyInstalled,
    PostFailed,
    Failed,
    Skipped,
}

impl Status {
    pub fn is_ok(self) -> bool {
        matches!(self, Status::Done | Status::AlreadyInstalled | Status::PostFailed)
    }

    pub fn is_finished(self) -> bool {
        !matches!(self, Status::Pending | Status::Running)
    }
}

#[derive(Clone)]
pub struct Job {
    pub tag: &'static str,
    pub name: String,
    pub deps: Vec<usize>,
    pub skip: bool,
    pub needs_sudo: bool,
    pub install: String,
    pub post: Vec<String>,
    pub marker: Option<PathBuf>,
}

pub enum Event {
    Started(usize),
    Line(usize, String),
    Finished(usize, Status),
    NeedSudo,
    AllDone,
}

pub struct Context {
    pub config_dir: PathBuf,
    pub dry_run: bool,
    pub log_file: PathBuf,
    pub shim_dir: PathBuf,
}

#[derive(Clone)]
struct Logger {
    tx: Sender<Event>,
    file: Option<Arc<Mutex<File>>>,
}

impl Logger {
    fn line(&self, job: usize, text: String) {
        if let Some(file) = &self.file
            && let Ok(mut f) = file.lock()
        {
            let _ = writeln!(f, "[{job}] {text}");
        }
        let _ = self.tx.send(Event::Line(job, text));
    }
}

pub fn spawn(jobs: Vec<Job>, ctx: Context, tx: Sender<Event>, sudo_rx: Receiver<bool>) {
    std::thread::spawn(move || run(jobs, ctx, tx, sudo_rx));
}

fn run(jobs: Vec<Job>, ctx: Context, tx: Sender<Event>, sudo_rx: Receiver<bool>) {
    if let Some(dir) = ctx.log_file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = File::create(&ctx.log_file).ok().map(|f| Arc::new(Mutex::new(f)));
    let log = Logger { tx: tx.clone(), file };
    if let Err(e) = sudo::write_shim(&ctx.shim_dir) {
        log.line(0, format!("kunde inte skapa sudo-shim: {e}"));
    }

    let mut statuses = vec![Status::Pending; jobs.len()];
    let mut sudo_ok = ctx.dry_run;
    let finish = |statuses: &mut Vec<Status>, i: usize, s: Status| {
        statuses[i] = s;
        let _ = tx.send(Event::Finished(i, s));
    };

    for (i, job) in jobs.iter().enumerate() {
        if job.deps.iter().any(|&d| !statuses[d].is_ok()) {
            log.line(i, "hoppas över: ett beroende misslyckades".into());
            finish(&mut statuses, i, Status::Skipped);
            continue;
        }
        if job.skip {
            finish(&mut statuses, i, Status::AlreadyInstalled);
            continue;
        }
        if job.needs_sudo && !sudo_ok {
            sudo_ok = sudo::cached() || {
                let _ = tx.send(Event::NeedSudo);
                sudo_rx.recv().unwrap_or(false)
            };
            if !sudo_ok {
                for j in i..jobs.len() {
                    finish(&mut statuses, j, Status::Skipped);
                }
                break;
            }
        }

        statuses[i] = Status::Running;
        let _ = tx.send(Event::Started(i));
        log.line(i, format!("== {} ==", job.name));

        if !run_command(i, &job.install, &ctx, &log) {
            finish(&mut statuses, i, Status::Failed);
            continue;
        }
        if let Some(marker) = &job.marker
            && !ctx.dry_run
        {
            if let Some(dir) = marker.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = File::create(marker);
        }
        let post_ok = job.post.iter().all(|cmd| run_command(i, cmd, &ctx, &log));
        finish(&mut statuses, i, if post_ok { Status::Done } else { Status::PostFailed });
    }
    let _ = tx.send(Event::AllDone);
}

fn run_command(job: usize, cmd: &str, ctx: &Context, log: &Logger) -> bool {
    log.line(job, format!("$ {cmd}"));
    if ctx.dry_run {
        std::thread::sleep(Duration::from_millis(80));
        return true;
    }
    let path = format!(
        "{}:{}",
        ctx.shim_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let child = Command::new("bash")
        .args(["-c", cmd])
        .current_dir(&ctx.config_dir)
        .env("PATH", path)
        .env("CONFIG_PATH", &ctx.config_dir)
        .env("FILES", ctx.config_dir.join("files"))
        .env("GIT_SSH_COMMAND", "ssh -o StrictHostKeyChecking=accept-new -o BatchMode=yes")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            log.line(job, format!("kunde inte starta: {e}"));
            return false;
        }
    };
    let readers = [
        child.stdout.take().map(|s| pipe(job, s, log.clone())),
        child.stderr.take().map(|s| pipe(job, s, log.clone())),
    ];
    let status = child.wait();
    for r in readers.into_iter().flatten() {
        let _ = r.join();
    }
    match status {
        Ok(s) if s.success() => true,
        Ok(s) => {
            log.line(job, format!("avslutades med {s}"));
            false
        }
        Err(e) => {
            log.line(job, format!("fel: {e}"));
            false
        }
    }
}

fn pipe(job: usize, src: impl Read + Send + 'static, log: Logger) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(src);
        let mut buf = Vec::new();
        while reader.read_until(b'\n', &mut buf).unwrap_or(0) > 0 {
            let text = String::from_utf8_lossy(&buf);
            // Progress bars redraw with \r; keep only the final state of the line.
            let line = text.trim_end().rsplit('\r').find(|s| !s.trim().is_empty()).unwrap_or("");
            if !line.is_empty() {
                log.line(job, strip_ansi(line));
            }
            buf.clear();
        }
    })
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
