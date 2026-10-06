use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Once;
use std::time::Duration;

pub fn cached() -> bool {
    quiet(Command::new("sudo").args(["-n", "true"]))
}

pub fn validate(password: &str) -> bool {
    let Ok(mut child) = Command::new("sudo")
        .args(["-S", "-p", "", "-v"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{password}");
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

pub fn start_keepalive() {
    static START: Once = Once::new();
    START.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(60));
            quiet(Command::new("sudo").args(["-n", "-v"]));
        });
    });
}

/// A `sudo` wrapper placed first in PATH so child processes never prompt on the TUI's terminal.
pub fn write_shim(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("sudo");
    std::fs::write(&path, "#!/bin/sh\nexec /usr/bin/sudo -n \"$@\"\n")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
}

fn quiet(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
