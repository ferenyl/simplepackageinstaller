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
            "--config" => config_path = Some(PathBuf::from(args.next().context("--config requires a path")?)),
            "-h" | "--help" => {
                println!("simplepackageinstaller [--config packages.yaml] [--dry-run] [--version]");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("simplepackageinstaller {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    let config_path = match config_path {
        Some(p) => p,
        None => find_config().context(
            "packages.yaml not found in the current directory or $XDG_CONFIG_HOME/simplepackageinstaller, use --config",
        )?,
    };
    let config_path = config_path.canonicalize()?;
    let config_dir = config_path.parent().context("config has no parent directory")?.to_path_buf();
    let cfg = Config::load(&config_path)?;

    let _sudo = sudo::Session;
    let mut terminal = ratatui::init();
    let result = ui::App::new(cfg, config_dir, dry_run).run(&mut terminal);
    ratatui::restore();
    result
}

/// `./packages.yaml`, then `$XDG_CONFIG_HOME/simplepackageinstaller/packages.yaml`.
fn find_config() -> Option<PathBuf> {
    [PathBuf::from("packages.yaml"), state::user_config_dir().join("packages.yaml")].into_iter().find(|p| p.is_file())
}
