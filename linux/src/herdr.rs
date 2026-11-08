use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PinChange {
    On,
    Off,
    Toggle,
}

impl PinChange {
    pub fn parse(word: &str) -> Option<PinChange> {
        match word.trim() {
            "on" => Some(PinChange::On),
            "off" => Some(PinChange::Off),
            "toggle" | "" => Some(PinChange::Toggle),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            PinChange::On => "on",
            PinChange::Off => "off",
            PinChange::Toggle => "toggle",
        }
    }

    pub fn apply(self, pinned: bool) -> bool {
        match self {
            PinChange::On => true,
            PinChange::Off => false,
            PinChange::Toggle => !pinned,
        }
    }
}

pub fn command(line: &str) -> Option<PinChange> {
    line.trim().strip_prefix("pin").and_then(PinChange::parse)
}

pub fn pin_file(home: &Path) -> PathBuf {
    home.join(".local/state/sth-dictate/pinned")
}

pub fn remember(file: &Path, pinned: bool) {
    if !pinned {
        let _ = std::fs::remove_file(file);
        return;
    }
    let _ = file.parent().map(std::fs::create_dir_all);
    let _ = std::fs::write(file, "pinned to herdr\n");
}

pub fn pane_id(json: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(json)
        .ok()?
        .pointer("/result/pane/pane_id")?
        .as_str()
        .map(str::to_string)
}

fn herdr(args: &[&str]) -> Result<Output, String> {
    Command::new("herdr")
        .args(args)
        .env_remove("HERDR_PANE_ID")
        .output()
        .map_err(|error| format!("Could not run herdr: {error}"))
}

fn focused_pane() -> Result<String, String> {
    let current = herdr(&["pane", "current"])?;
    pane_id(&current.stdout).ok_or_else(|| "Herdr has no focused pane".to_string())
}

pub fn send(text: &str) -> Result<(), String> {
    let pane = focused_pane()?;
    let sent = herdr(&["pane", "send-text", &pane, text])?;
    if sent.status.success() {
        return Ok(());
    }
    Err(format!("herdr could not send to pane {pane}: {}", String::from_utf8_lossy(&sent.stderr).trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_command_names_its_change() {
        assert_eq!(command("pin on\n"), Some(PinChange::On));
        assert_eq!(command("pin off"), Some(PinChange::Off));
        assert_eq!(command("pin toggle"), Some(PinChange::Toggle));
        assert_eq!(command("pin"), Some(PinChange::Toggle));
        assert_eq!(command("pin sideways"), None);
        assert_eq!(command("hello"), None);
    }

    #[test]
    fn a_change_sets_or_flips_the_pin() {
        assert!(PinChange::On.apply(false));
        assert!(!PinChange::Off.apply(true));
        assert!(PinChange::Toggle.apply(false));
        assert!(!PinChange::Toggle.apply(true));
    }

    #[test]
    fn the_focused_pane_comes_from_herdr_pane_current() {
        let answer =
            br#"{"id":"cli:pane:current","result":{"pane":{"focused":true,"pane_id":"wB:p1"},"type":"pane_current"}}"#;
        assert_eq!(pane_id(answer), Some("wB:p1".into()));
        assert_eq!(pane_id(b"{\"error\":\"no server\"}"), None);
    }

    #[test]
    fn the_pin_is_remembered_in_a_file() {
        let dir = std::env::temp_dir().join(format!("sth-dictate-pin-{}", std::process::id()));
        let file = pin_file(&dir);
        remember(&file, true);
        assert!(file.exists());
        remember(&file, false);
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
