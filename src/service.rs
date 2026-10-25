use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{config_dir, data_dir};

/// systemd user unit that runs `rc-tui watch` in the background.
pub const UNIT: &str = "rc-tui-watch.service";
#[cfg(any(target_os = "macos", test))]
const LABEL: &str = "com.guitaripod.rc-tui.watch";
const DESKTOP_FILE: &str = "rc-tui.desktop";
const ICON_FILE: &str = "rc-tui.svg";
#[cfg(any(target_os = "macos", test))]
const WATCH_LOG_FILE: &str = "rc-tui-watch.log";

/// The 256x256 app icon: a dark rounded square with a green glow, a rim and a four-pointed sparkle.
pub const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
  <defs>
    <radialGradient id="glow" cx="24" cy="24" r="220" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#3fd07f" stop-opacity="0.3"/>
      <stop offset="1" stop-color="#3fd07f" stop-opacity="0"/>
    </radialGradient>
  </defs>
  <rect width="256" height="256" rx="56" fill="#0b0d11"/>
  <rect width="256" height="256" rx="56" fill="url(#glow)"/>
  <rect x="2" y="2" width="252" height="252" rx="54" fill="none" stroke="#3fd07f" stroke-width="4" stroke-opacity="0.55"/>
  <path d="M128 53 Q134 122 203 128 Q134 134 128 203 Q122 134 53 128 Q122 122 128 53 Z" fill="#3fd07f"/>
</svg>
"##;

/// Installs the background watcher for this platform and returns one line per step taken.
#[cfg(target_os = "linux")]
pub fn install(exe: &Path) -> io::Result<Vec<String>> {
    let config = config_dir();
    let data = data_dir();
    let unit = unit_path(&config);
    let desktop = desktop_path(&data);
    let icon = icon_path(&data);
    let mut lines = Vec::new();

    write_file(&unit, &unit_text(exe))?;
    lines.push(format!("wrote {}", tilde(&unit)));
    write_file(&desktop, &desktop_text(exe))?;
    lines.push(format!("wrote {}", tilde(&desktop)));
    write_file(&icon, ICON_SVG)?;
    lines.push(format!("wrote {}", tilde(&icon)));

    run_checked("systemctl", &["--user", "daemon-reload"])?;
    run_checked("systemctl", &["--user", "enable", UNIT])?;
    lines.push(format!("enabled {UNIT}"));
    run_checked("systemctl", &["--user", "restart", UNIT])?;
    lines.push(format!("restarted {UNIT}"));
    Ok(lines)
}

/// Installs the background watcher for this platform and returns one line per step taken.
#[cfg(target_os = "macos")]
pub fn install(exe: &Path) -> io::Result<Vec<String>> {
    let home = home_dir();
    let plist = launch_agent_path(&home);
    let log = watch_log_path(&home);
    let mut lines = Vec::new();

    write_file(&plist, &plist_text(exe, &log))?;
    lines.push(format!("wrote {}", tilde(&plist)));

    let domain = gui_domain();
    bootout_quietly(&domain);
    let plist_arg = plist.display().to_string();
    run_checked(
        "launchctl",
        &["bootstrap", domain.as_str(), plist_arg.as_str()],
    )?;
    lines.push(format!("loaded {LABEL}"));
    Ok(lines)
}

/// Installs the background watcher for this platform and returns one line per step taken.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn install(_exe: &Path) -> io::Result<Vec<String>> {
    Err(unsupported())
}

/// Stops the background watcher and removes every file that `install` wrote.
#[cfg(target_os = "linux")]
pub fn uninstall() -> io::Result<Vec<String>> {
    let config = config_dir();
    let data = data_dir();
    let mut lines = Vec::new();

    if run_checked("systemctl", &["--user", "disable", "--now", UNIT]).is_ok() {
        lines.push(format!("disabled and stopped {UNIT}"));
    }
    for path in [unit_path(&config), desktop_path(&data), icon_path(&data)] {
        if remove_if_present(&path)? {
            lines.push(format!("removed {}", tilde(&path)));
        }
    }
    run_checked("systemctl", &["--user", "daemon-reload"])?;
    Ok(lines)
}

/// Stops the background watcher and removes every file that `install` wrote.
#[cfg(target_os = "macos")]
pub fn uninstall() -> io::Result<Vec<String>> {
    let plist = launch_agent_path(&home_dir());
    let mut lines = Vec::new();

    bootout_quietly(&gui_domain());
    lines.push(format!("unloaded {LABEL}"));
    if remove_if_present(&plist)? {
        lines.push(format!("removed {}", tilde(&plist)));
    }
    Ok(lines)
}

/// Stops the background watcher and removes every file that `install` wrote.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn uninstall() -> io::Result<Vec<String>> {
    Err(unsupported())
}

/// The watcher's state for the status line: systemd's `is-active` word, or "loaded" or "not loaded" on launchd.
#[cfg(target_os = "linux")]
pub fn status() -> Option<String> {
    let output = Command::new("systemctl")
        .args(["--user", "is-active", UNIT])
        .output()
        .ok()?;
    let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!state.is_empty()).then_some(state)
}

/// The watcher's state for the status line: systemd's `is-active` word, or "loaded" or "not loaded" on launchd.
#[cfg(target_os = "macos")]
pub fn status() -> Option<String> {
    let loaded = Command::new("launchctl")
        .args(["print", &format!("{}/{LABEL}", gui_domain())])
        .output()
        .is_ok_and(|output| output.status.success());
    Some(if loaded { "loaded" } else { "not loaded" }.to_string())
}

/// The watcher's state for the status line: systemd's `is-active` word, or "loaded" or "not loaded" on launchd.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn status() -> Option<String> {
    None
}

/// Error returned by the install and uninstall functions on platforms without a supported service manager.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported() -> io::Error {
    io::Error::other("rc-tui watch --install supports Linux (systemd) and macOS (launchd)")
}

/// The systemd user unit: runs `watch` at login, restarts it after failures and lowers its CPU priority.
fn unit_text(exe: &Path) -> String {
    let exec = systemd_quote(&exe.to_string_lossy());
    format!(
        "[Unit]\n\
         Description=rc-tui moments: records, milestones and firsts from RevenueCat\n\
         After=network-online.target graphical-session.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart=\"{exec}\" watch\n\
         Restart=on-failure\n\
         RestartSec=30\n\
         Nice=10\n\
         \n\
         [Install]\n\
         WantedBy=graphical-session.target\n"
    )
}

/// The freedesktop launcher entry that starts the dashboard in a terminal.
fn desktop_text(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=rc-tui\n\
         GenericName=App revenue dashboard\n\
         Comment=People, revenue and moments from RevenueCat\n\
         Exec={exec}\n\
         Icon=rc-tui\n\
         Terminal=true\n\
         Categories=Office;Finance;\n\
         Keywords=revenuecat;revenue;sales;apps;\n\
         StartupNotify=false\n",
        exec = desktop_exec(exe)
    )
}

#[cfg(any(target_os = "macos", test))]
/// The launchd agent that runs `watch` at login, keeps it alive after failures and logs both streams to `log`.
fn plist_text(exe: &Path, log: &Path) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    let log = xml_escape(&log.to_string_lossy());
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>watch</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"##
    )
}

/// Quotes an `ExecStart=` argument for systemd, escaping the quote, backslash, `%` specifier and `$` variable characters.
fn systemd_quote(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$")
}

/// Formats an `Exec=` value: `%` is doubled, and a path with spaces is quoted with its reserved characters escaped.
fn desktop_exec(exe: &Path) -> String {
    let path = exe.to_string_lossy().replace('%', "%%");
    if !path.contains(' ') {
        return path;
    }
    let escaped = path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    format!("\"{escaped}\"")
}

#[cfg(any(target_os = "macos", test))]
/// Escapes the five XML special characters so a path can sit inside a plist `<string>`.
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Where the systemd user unit lives under the XDG config directory.
fn unit_path(config: &Path) -> PathBuf {
    config.join("systemd").join("user").join(UNIT)
}

/// Where the launcher entry lives under the XDG data directory.
fn desktop_path(data: &Path) -> PathBuf {
    data.join("applications").join(DESKTOP_FILE)
}

/// Where the hicolor theme icon lives under the XDG data directory.
fn icon_path(data: &Path) -> PathBuf {
    data.join("icons/hicolor/scalable/apps").join(ICON_FILE)
}

#[cfg(any(target_os = "macos", test))]
/// Where the launchd agent plist lives under `~/Library/LaunchAgents`.
fn launch_agent_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

#[cfg(any(target_os = "macos", test))]
/// Where launchd appends the watcher's stdout and stderr, under `~/Library/Logs`.
fn watch_log_path(home: &Path) -> PathBuf {
    home.join("Library/Logs").join(WATCH_LOG_FILE)
}

/// The launchd domain for the current user, such as `gui/501`.
#[cfg(target_os = "macos")]
fn gui_domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

/// Unloads the agent and ignores the failure, which just means it was not loaded.
#[cfg(target_os = "macos")]
fn bootout_quietly(domain: &str) {
    let _ = Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .output();
}

/// Prints a path with the home directory shortened to `~`, for the report lines.
fn tilde(path: &Path) -> String {
    tilde_under(path, &home_dir())
}

/// Shortens `path` to `~/...` when it sits under `home`, and leaves it unchanged otherwise.
fn tilde_under(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// The current user's home directory from `$HOME`, falling back to the working directory.
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Writes `text` to `path`, creating the parent directories first.
fn write_file(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)
}

/// Deletes `path` and reports whether it existed.
fn remove_if_present(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Runs a service-manager command and fails with its name and stderr when it does not succeed.
fn run_checked(program: &str, args: &[&str]) -> io::Result<()> {
    let output = Command::new(program).args(args).output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = match stderr.trim() {
        "" => output.status.to_string(),
        text => text.to_string(),
    };
    Err(io::Error::other(format!(
        "{program} {} failed: {detail}",
        args.join(" ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_runs_quoted_exe_as_watch_and_restarts_on_failure() {
        let text = unit_text(Path::new("/opt/rc tui/rc-tui"));
        assert!(text.contains("ExecStart=\"/opt/rc tui/rc-tui\" watch\n"));
        assert!(text.contains("Restart=on-failure\n"));
        assert!(text.contains("WantedBy=graphical-session.target\n"));
        assert!(text.contains("After=network-online.target graphical-session.target\n"));
    }

    #[test]
    fn unit_escapes_systemd_specials_in_exe_path() {
        let text = unit_text(Path::new("/x/50%$ \"q\"/rc-tui"));
        assert!(text.contains(r#"ExecStart="/x/50%%$$ \"q\"/rc-tui" watch"#));
    }

    #[test]
    fn desktop_quotes_exec_only_when_path_has_spaces() {
        let spaced = desktop_text(Path::new("/opt/rc tui/rc-tui"));
        assert!(spaced.contains("\nExec=\"/opt/rc tui/rc-tui\"\n"));
        let plain = desktop_text(Path::new("/usr/bin/rc-tui"));
        assert!(plain.contains("\nExec=/usr/bin/rc-tui\n"));
        assert!(plain.contains("Terminal=true\n"));
        assert!(plain.contains("Icon=rc-tui\n"));
    }

    #[test]
    fn plist_has_label_arguments_and_escaped_paths() {
        let text = plist_text(
            Path::new("/srv/R&D/rc-tui"),
            Path::new("/home/u/Library/Logs/rc-tui-watch.log"),
        );
        assert!(text.contains("<string>com.guitaripod.rc-tui.watch</string>"));
        assert!(text.contains("<string>/srv/R&amp;D/rc-tui</string>"));
        assert!(text.contains("<string>watch</string>"));
        assert!(!text.contains("R&D"));
        assert_eq!(
            text.matches("<string>/home/u/Library/Logs/rc-tui-watch.log</string>")
                .count(),
            2
        );
        assert_eq!(
            text.matches("<key>").count(),
            text.matches("</key>").count()
        );
        assert_eq!(text.matches("<key>").count(), 8);
    }

    #[test]
    fn xml_escape_covers_every_special_character() {
        assert_eq!(
            xml_escape(r#"<a & 'b' "c">"#),
            "&lt;a &amp; &apos;b&apos; &quot;c&quot;&gt;"
        );
    }

    #[test]
    fn paths_follow_xdg_and_library_layout() {
        assert_eq!(
            unit_path(Path::new("/c")),
            PathBuf::from("/c/systemd/user/rc-tui-watch.service")
        );
        assert_eq!(
            desktop_path(Path::new("/d")),
            PathBuf::from("/d/applications/rc-tui.desktop")
        );
        assert_eq!(
            icon_path(Path::new("/d")),
            PathBuf::from("/d/icons/hicolor/scalable/apps/rc-tui.svg")
        );
        assert_eq!(
            launch_agent_path(Path::new("/h")),
            PathBuf::from("/h/Library/LaunchAgents/com.guitaripod.rc-tui.watch.plist")
        );
        assert_eq!(
            watch_log_path(Path::new("/h")),
            PathBuf::from("/h/Library/Logs/rc-tui-watch.log")
        );
    }

    #[test]
    fn tilde_abbreviates_only_paths_under_home() {
        assert_eq!(
            tilde_under(Path::new("/home/u/.config/x"), Path::new("/home/u")),
            "~/.config/x"
        );
        assert_eq!(
            tilde_under(Path::new("/home/user/x"), Path::new("/home/u")),
            "/home/user/x"
        );
    }

    #[test]
    fn icon_is_an_svg_with_a_path() {
        assert!(ICON_SVG.trim_start().starts_with("<svg"));
        assert!(ICON_SVG.contains("<path"));
    }

    #[test]
    #[ignore]
    fn writes_icon_to_disk() {
        if let Some(path) = std::env::var_os("RC_TUI_ICON_OUT") {
            fs::write(path, ICON_SVG).unwrap();
        }
    }
}
