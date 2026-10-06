use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

use crate::config::Package;
use crate::source::Source;

pub struct InstalledState {
    packages: HashSet<String>,
    groups: HashSet<String>,
    flatpaks: HashSet<String>,
    npm: HashSet<String>,
    cargo: HashSet<String>,
    uv: HashSet<String>,
    dotnet: HashSet<String>,
    marker_dir: PathBuf,
}

impl InstalledState {
    pub fn load() -> Self {
        let groups = lines("pacman", &["-Qg"])
            .into_iter()
            .filter_map(|l| l.split_whitespace().next().map(str::to_string))
            .collect();
        Self {
            packages: lines("pacman", &["-Qq"]).into_iter().collect(),
            groups,
            flatpaks: lines("flatpak", &["list", "--columns=application"]).into_iter().collect(),
            npm: npm_packages(),
            cargo: first_words("cargo", &["install", "--list"], |l| !l.starts_with(' ')),
            uv: first_words("uv", &["tool", "list"], |l| !l.starts_with('-')),
            dotnet: dotnet_tools(),
            marker_dir: marker_dir(),
        }
    }

    pub fn is_installed(&self, pkg: &Package) -> bool {
        match pkg.source {
            Source::Pacman | Source::Paru => self.packages.contains(&pkg.name) || self.groups.contains(&pkg.name),
            Source::Flatpak => self.flatpaks.contains(&pkg.name),
            Source::Npm => self.npm.contains(&pkg.name),
            Source::Cargo => self.cargo.contains(&pkg.name),
            Source::Uv => self.uv.contains(&pkg.name),
            Source::Dotnet => self.dotnet.contains(&pkg.name.to_lowercase()),
            // A script may install a real package with the same name (e.g. paru).
            Source::Script if pkg.always => false,
            Source::Script => self.marker(&pkg.name).exists() || self.packages.contains(&pkg.name),
        }
    }

    pub fn marker(&self, name: &str) -> PathBuf {
        self.marker_dir.join(format!("{name}.done"))
    }
}

fn marker_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state"));
    base.join("simplearchpackageinstaller")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn npm_packages() -> HashSet<String> {
    lines("npm", &["ls", "-g", "--depth=0", "--parseable"])
        .into_iter()
        .filter_map(|l| l.split_once("node_modules/").map(|(_, name)| name.to_string()))
        .collect()
}

fn dotnet_tools() -> HashSet<String> {
    let local = home().join(".dotnet/dotnet");
    let dotnet = if local.exists() { local.to_string_lossy().into_owned() } else { "dotnet".into() };
    // Output: header, dashed separator, then "<id>  <version>  <commands>" rows.
    lines(&dotnet, &["tool", "list", "-g"])
        .into_iter()
        .skip(2)
        .filter_map(|l| l.split_whitespace().next().map(str::to_lowercase))
        .collect()
}

fn first_words(cmd: &str, args: &[&str], keep: fn(&str) -> bool) -> HashSet<String> {
    lines_raw(cmd, args)
        .into_iter()
        .filter(|l| !l.trim().is_empty() && keep(l))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect()
}

fn lines(cmd: &str, args: &[&str]) -> Vec<String> {
    lines_raw(cmd, args).into_iter().map(|l| l.trim().to_string()).collect()
}

fn lines_raw(cmd: &str, args: &[&str]) -> Vec<String> {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect())
        .unwrap_or_default()
}
