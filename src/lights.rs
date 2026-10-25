use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const PULSE_MODES: [&str; 6] = [
    "Breathing",
    "Color Pulse",
    "Pulse",
    "Flashing",
    "Static",
    "Direct",
];
const SAVED_PROFILE: &str = "rc-tui-before";

/// RGB hardware through the OpenRGB CLI: a slow pulse in the moment's colour, then back to how it was.
#[derive(Clone)]
pub struct Lights {
    profile: Option<String>,
    busy: Arc<AtomicBool>,
}

impl Lights {
    /// Present when `openrgb` is installed; `profile` is what to load afterwards, else the state is saved first.
    pub fn detect(profile: Option<String>) -> Option<Self> {
        let installed = Command::new("openrgb")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok();
        installed.then(|| Self {
            profile: profile.filter(|name| !name.trim().is_empty()),
            busy: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Pulses every device for `hold`, on a thread of its own; a pulse already running wins.
    pub fn pulse(&self, rgb: (u8, u8, u8), hold: Duration) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let lights = self.clone();
        std::thread::spawn(move || {
            lights.run(rgb, hold);
            lights.busy.store(false, Ordering::SeqCst);
        });
    }

    fn run(&self, rgb: (u8, u8, u8), hold: Duration) {
        let Ok(listing) = Command::new("openrgb").arg("--list-devices").output() else {
            return;
        };
        let color = format!("{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2);
        let args = pulse_args(&String::from_utf8_lossy(&listing.stdout), &color);
        if args.is_empty() {
            return;
        }
        let restore = match &self.profile {
            Some(profile) => profile.clone(),
            None => {
                if !openrgb(&["--save-profile", SAVED_PROFILE]) {
                    return;
                }
                SAVED_PROFILE.to_string()
            }
        };
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        if openrgb(&refs) {
            std::thread::sleep(hold);
        }
        openrgb(&["--profile", &restore]);
    }
}

fn openrgb(args: &[&str]) -> bool {
    Command::new("openrgb")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// One `-d N -m MODE -c COLOR` group per device, picking the gentlest pulse each device supports.
fn pulse_args(listing: &str, color: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut device: Option<String> = None;
    for line in listing.lines() {
        if let Some((index, _)) = line.split_once(':')
            && !line.starts_with(' ')
            && index.trim().parse::<usize>().is_ok()
        {
            device = Some(index.trim().to_string());
            continue;
        }
        let Some(modes) = line.trim().strip_prefix("Modes:") else {
            continue;
        };
        let Some(index) = device.take() else {
            continue;
        };
        let modes = mode_names(modes);
        if let Some(mode) = PULSE_MODES
            .iter()
            .find(|wanted| modes.iter().any(|mode| mode.eq_ignore_ascii_case(wanted)))
        {
            args.extend([
                "--device".to_string(),
                index,
                "--mode".to_string(),
                mode.to_string(),
                "--color".to_string(),
                color.to_string(),
            ]);
        }
    }
    args
}

/// Splits `[Direct] Custom 'Color Shift'` into its mode names.
fn mode_names(modes: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in modes.chars() {
        match character {
            '\'' => {
                quoted = !quoted;
                if !quoted && !current.is_empty() {
                    names.push(std::mem::take(&mut current));
                }
            }
            ' ' if !quoted => {
                if !current.is_empty() {
                    names.push(std::mem::take(&mut current));
                }
            }
            '[' | ']' if !quoted => {}
            _ => current.push(character),
        }
    }
    if !current.is_empty() {
        names.push(current);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "0: Corsair Dominator Platinum RGB DDR5\n  Type:           DRAM\n  Modes: [Direct] Custom 'Color Shift' 'Color Pulse' 'Rainbow Wave' Visor\n  Zones: 'Corsair DRAM'\n\n1: Z790 AORUS ELITE AX\n  Type:           Motherboard\n  Modes: [Direct] Static Breathing Flashing 'Color Cycle'\n\n2: Mystery Strip\n  Modes: Rainbow\n";

    #[test]
    fn reads_quoted_mode_names() {
        assert_eq!(
            mode_names(" [Direct] Custom 'Color Shift' 'Color Pulse'"),
            vec!["Direct", "Custom", "Color Shift", "Color Pulse"]
        );
    }

    #[test]
    fn picks_the_gentlest_pulse_per_device_and_skips_the_rest() {
        let args = pulse_args(LISTING, "3FD07F");
        assert_eq!(
            args,
            vec![
                "--device",
                "0",
                "--mode",
                "Color Pulse",
                "--color",
                "3FD07F",
                "--device",
                "1",
                "--mode",
                "Breathing",
                "--color",
                "3FD07F",
            ]
        );
    }
}
