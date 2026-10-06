mod config;
mod deps;
mod runner;
mod source;
mod state;
mod sudo;
mod ui;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use config::Config;

fn main() -> Result<()> {
    let mut config_path = None;
    let mut dry_run = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--config" => config_path = Some(PathBuf::from(args.next().context("--config kräver en sökväg")?)),
            "-h" | "--help" => {
                println!("simplearchpackageinstaller [--config packages.yaml] [--dry-run]");
                return Ok(());
            }
            other => bail!("okänt argument: {other}"),
        }
    }

    let config_path = match config_path {
        Some(p) => p,
        None => find_config().context("hittar inte packages.yaml, ange --config")?,
    };
    let config_path = config_path.canonicalize()?;
    let config_dir = config_path.parent().context("config saknar katalog")?.to_path_buf();
    let cfg = Config::load(&config_path)?;

    let mut terminal = ratatui::init();
    let result = ui::App::new(cfg, config_dir, dry_run).run(&mut terminal);
    ratatui::restore();
    result
}

fn find_config() -> Option<PathBuf> {
    let path = PathBuf::from("packages.yaml");
    path.is_file().then_some(path)
}
