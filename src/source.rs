use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::Package;
use crate::state::home;

/// Variant order is the install order within a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Pacman,
    Paru,
    Flatpak,
    Npm,
    Cargo,
    Uv,
    Dotnet,
    Script,
}

const DOTNET: &str = r#""$(command -v dotnet || echo "$HOME/.dotnet/dotnet")""#;

impl Source {
    pub fn tag(self) -> &'static str {
        match self {
            Source::Pacman => "pacman",
            Source::Paru => "paru",
            Source::Flatpak => "flatpak",
            Source::Npm => "npm",
            Source::Cargo => "cargo",
            Source::Uv => "uv",
            Source::Dotnet => "dotnet",
            Source::Script => "script",
        }
    }

    /// Package (by name) that provides the installer, if it is defined in the config.
    pub fn implicit_requirement(self) -> Option<&'static str> {
        match self {
            Source::Paru => Some("paru"),
            Source::Flatpak => Some("flatpak"),
            Source::Npm => Some("npm"),
            Source::Cargo => Some("rust"),
            Source::Uv => Some("uv"),
            Source::Dotnet => Some("dotnet"),
            Source::Pacman | Source::Script => None,
        }
    }
}

pub fn script_path(config_dir: &Path, pkg: &Package) -> PathBuf {
    let file = pkg.script_file();
    match file.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        // join() keeps absolute paths as they are.
        None => config_dir.join(file),
    }
}

pub fn install_command(pkg: &Package, config_dir: &Path) -> String {
    let flags = pkg.flags.join(" ");
    let name = &pkg.name;
    let cmd = match pkg.source {
        Source::Pacman => format!("sudo pacman -S --needed --noconfirm {flags} {name}"),
        Source::Paru => format!("paru -S --needed --noconfirm --sudoloop {flags} {name}"),
        Source::Flatpak => format!("flatpak install -y --noninteractive {flags} flathub {name}"),
        Source::Npm => format!("sudo npm install -g {flags} {name}"),
        Source::Cargo => format!("cargo install --locked {flags} {name}"),
        Source::Uv => format!("uv tool install {flags} {name}"),
        Source::Dotnet => format!("{DOTNET} tool update -g {flags} {name}"),
        Source::Script => format!(
            "bash -euo pipefail {} {flags}",
            shell_quote(&script_path(config_dir, pkg).to_string_lossy())
        ),
    };
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn post_commands(pkg: &Package) -> Vec<String> {
    let mut cmds = Vec::new();
    if !pkg.service.is_empty() {
        cmds.push(format!("sudo systemctl enable --now {}", pkg.service.join(" ")));
    }
    if !pkg.user_service.is_empty() {
        cmds.push(format!("systemctl --user enable {}", pkg.user_service.join(" ")));
    }
    cmds.extend(pkg.post.iter().cloned());
    cmds
}

pub fn needs_sudo(pkg: &Package, config_dir: &Path) -> bool {
    let install = match pkg.source {
        Source::Pacman | Source::Paru | Source::Npm => true,
        Source::Flatpak | Source::Cargo | Source::Uv | Source::Dotnet => false,
        Source::Script => std::fs::read_to_string(script_path(config_dir, pkg))
            .map(|s| s.contains("sudo"))
            .unwrap_or(false),
    };
    install || post_commands(pkg).iter().any(|c| c.contains("sudo"))
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
