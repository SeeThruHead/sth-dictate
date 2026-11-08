use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    Herdr,
    Window { address: String, class: String, title: String },
}

impl Target {
    pub fn label(&self) -> String {
        match self {
            Target::Herdr => "Herdr".into(),
            Target::Window { class, title, .. } if class.is_empty() => title.clone(),
            Target::Window { class, .. } => class.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pin {
    pub on: bool,
    pub target: Option<Target>,
}

impl Pin {
    pub fn target(&self) -> Option<Target> {
        self.on.then(|| self.target.clone().unwrap_or(Target::Herdr))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PinChange {
    On,
    Off,
    Toggle,
    To(Target),
}

impl PinChange {
    pub fn apply(self, pin: Pin) -> Pin {
        match self {
            PinChange::On => Pin { on: true, ..pin },
            PinChange::Off => Pin { on: false, ..pin },
            PinChange::Toggle => Pin { on: !pin.on, ..pin },
            PinChange::To(target) => Pin { on: true, target: Some(target) },
        }
    }

    pub fn line(&self) -> String {
        match self {
            PinChange::On => "pin on\n".into(),
            PinChange::Off => "pin off\n".into(),
            PinChange::Toggle => "pin toggle\n".into(),
            PinChange::To(target) => format!("pin to {}\n", serde_json::to_string(target).unwrap_or_default()),
        }
    }
}

pub fn command(line: &str) -> Option<PinChange> {
    match line.trim() {
        "pin on" => Some(PinChange::On),
        "pin off" => Some(PinChange::Off),
        "pin toggle" => Some(PinChange::Toggle),
        other => other.strip_prefix("pin to ").and_then(|json| serde_json::from_str(json).ok()).map(PinChange::To),
    }
}

pub fn file(home: &Path) -> PathBuf {
    home.join(".local/state/sth-dictate/pin.json")
}

pub fn load(file: &Path) -> Pin {
    std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn save(file: &Path, pin: &Pin) {
    let _ = file.parent().map(std::fs::create_dir_all);
    let _ = std::fs::write(file, serde_json::to_string_pretty(pin).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slack() -> Target {
        Target::Window { address: "0x55".into(), class: "Slack".into(), title: "general".into() }
    }

    #[test]
    fn every_change_travels_as_one_line() {
        [PinChange::On, PinChange::Off, PinChange::Toggle, PinChange::To(slack()), PinChange::To(Target::Herdr)]
            .into_iter()
            .for_each(|change| assert_eq!(command(&change.line()), Some(change)));
        assert_eq!(command("pin sideways"), None);
        assert_eq!(command("pin to {broken"), None);
    }

    #[test]
    fn pinning_on_with_no_app_chosen_means_herdr() {
        assert_eq!(PinChange::On.apply(Pin::default()).target(), Some(Target::Herdr));
        assert_eq!(Pin::default().target(), None);
    }

    #[test]
    fn off_then_on_keeps_the_chosen_app() {
        let pinned = PinChange::To(slack()).apply(Pin::default());
        let back = PinChange::On.apply(PinChange::Off.apply(pinned));
        assert_eq!(back.target(), Some(slack()));
        assert_eq!(back.target().map(|target| target.label()), Some("Slack".into()));
    }

    #[test]
    fn the_pin_is_kept_in_a_file() {
        let dir = std::env::temp_dir().join(format!("sth-dictate-pin-{}", std::process::id()));
        let pin = PinChange::To(slack()).apply(Pin::default());
        save(&file(&dir), &pin);
        assert_eq!(load(&file(&dir)), pin);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&file(&dir)), Pin::default());
    }
}
