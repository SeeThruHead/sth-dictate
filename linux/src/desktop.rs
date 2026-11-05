use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, KeyCode, KeyEvent};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Duration;

pub const DEFAULT_APP: &str = "default";
pub const DEFAULT_PASTE: &str = "shift+Insert";

const KEY_GAP: Duration = Duration::from_millis(12);
const KEYBOARD_KEYS: std::ops::RangeInclusive<u16> = 1..=248;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    Start,
    Stop,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    #[default]
    Paste,
    Type,
}

fn modifier(name: &str) -> Option<KeyCode> {
    match name.to_lowercase().as_str() {
        "shift" => Some(KeyCode::KEY_LEFTSHIFT),
        "ctrl" => Some(KeyCode::KEY_LEFTCTRL),
        "alt" => Some(KeyCode::KEY_LEFTALT),
        "super" => Some(KeyCode::KEY_LEFTMETA),
        _ => None,
    }
}

pub fn key_codes(combo: &str) -> Result<Vec<KeyCode>, String> {
    let parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let (key, modifiers) = parts.split_last().ok_or_else(|| format!("\"{combo}\" names no key"))?;
    if key.is_empty() {
        return Err(format!("\"{combo}\" names no key"));
    }
    let held: Vec<KeyCode> = modifiers
        .iter()
        .map(|name| {
            modifier(name).ok_or_else(|| {
                format!("\"{combo}\" has an unknown modifier \"{name}\" (use shift, ctrl, alt or super)")
            })
        })
        .collect::<Result<_, _>>()?;
    let pressed = KeyCode::from_str(&format!("KEY_{}", key.to_uppercase()))
        .map_err(|_| format!("\"{combo}\" has an unknown key \"{key}\""))?;
    Ok(held.into_iter().chain([pressed]).collect())
}

pub fn combo_for<'a>(class: &str, paste_keys: &'a BTreeMap<String, String>) -> &'a str {
    paste_keys
        .iter()
        .find(|(app, _)| app.eq_ignore_ascii_case(class))
        .or_else(|| paste_keys.get_key_value(DEFAULT_APP))
        .map_or(DEFAULT_PASTE, |(_, combo)| combo.as_str())
}

pub struct Keyboard(Mutex<VirtualDevice>);

impl Keyboard {
    pub fn create() -> Result<Keyboard, String> {
        let keys: AttributeSet<KeyCode> = KEYBOARD_KEYS.map(KeyCode::new).collect();
        VirtualDevice::builder()
            .and_then(|builder| builder.name("sth-dictate keyboard").with_keys(&keys))
            .and_then(|builder| builder.build())
            .map(|device| Keyboard(Mutex::new(device)))
            .map_err(|error| format!("Could not create the paste keyboard on /dev/uinput: {error}"))
    }

    fn press(&self, codes: &[KeyCode]) -> Result<(), String> {
        let mut device = self.0.lock().map_err(|_| "The paste keyboard is unusable".to_string())?;
        let downs = codes.iter().map(|code| (*code, 1));
        let ups = codes.iter().rev().map(|code| (*code, 0));
        downs.chain(ups).try_for_each(|(code, value)| {
            std::thread::sleep(KEY_GAP);
            device
                .emit(&[*KeyEvent::new(code, value)])
                .map_err(|error| format!("Could not press the paste keys: {error}"))
        })
    }
}

pub trait Desktop: Send {
    fn deliver(&self, text: &str) -> Result<(), String>;
    fn play(&self, sound: Sound);
}

pub struct Wayland {
    pub sounds: PathBuf,
    pub delivery: Delivery,
    pub paste_keys: BTreeMap<String, String>,
    pub keyboard: Keyboard,
}

impl Desktop for Wayland {
    fn deliver(&self, text: &str) -> Result<(), String> {
        match self.delivery {
            Delivery::Paste => self.paste(text),
            Delivery::Type => wtype(&["--", text]),
        }
    }

    fn play(&self, sound: Sound) {
        let name = match sound {
            Sound::Start => "Tink.wav",
            Sound::Stop => "Pop.wav",
        };
        let _ = Command::new("pw-play")
            .arg(self.sounds.join(name))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|mut child| std::thread::spawn(move || child.wait()));
    }
}

impl Wayland {
    fn paste(&self, text: &str) -> Result<(), String> {
        copy(text)?;
        let combo = combo_for(&focused_class(), &self.paste_keys);
        self.keyboard.press(&key_codes(combo)?)
    }
}

fn focused_class() -> String {
    Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()
        .and_then(|output| serde_json::from_slice::<serde_json::Value>(&output.stdout).ok())
        .and_then(|window| window.get("class").and_then(serde_json::Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

fn wtype(args: &[&str]) -> Result<(), String> {
    let typed = Command::new("wtype").args(args).status().map_err(|error| format!("Could not run wtype: {error}"))?;
    typed.success().then_some(()).ok_or_else(|| "wtype could not type the text".to_string())
}

fn copy(text: &str) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not run wl-copy: {error}"))?;
    child
        .stdin
        .take()
        .ok_or("wl-copy took no input")?
        .write_all(text.as_bytes())
        .map_err(|error| format!("Could not hand the text to wl-copy: {error}"))?;
    let copied = child.wait().map_err(|error| format!("wl-copy failed: {error}"))?;
    copied.success().then_some(()).ok_or_else(|| "wl-copy could not set the clipboard".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(app, combo)| (app.to_string(), combo.to_string())).collect()
    }

    #[test]
    fn a_combo_is_its_modifiers_then_its_key() {
        assert_eq!(key_codes("ctrl+shift+v").unwrap(), [KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_V]);
        assert_eq!(key_codes("Shift+Insert").unwrap(), [KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_INSERT]);
        assert_eq!(key_codes("super+space").unwrap(), [KeyCode::KEY_LEFTMETA, KeyCode::KEY_SPACE]);
    }

    #[test]
    fn a_bad_combo_says_what_is_wrong() {
        assert!(key_codes("ctlr+v").unwrap_err().contains("unknown modifier \"ctlr\""));
        assert!(key_codes("ctrl+").unwrap_err().contains("names no key"));
        assert!(key_codes("ctrl+banana").unwrap_err().contains("unknown key \"banana\""));
    }

    #[test]
    fn the_focused_app_picks_its_own_combo_or_the_default() {
        let paste_keys = keys(&[("default", "shift+Insert"), ("wow.exe", "ctrl+v")]);
        assert_eq!(combo_for("WoW.exe", &paste_keys), "ctrl+v");
        assert_eq!(combo_for("kitty", &paste_keys), "shift+Insert");
        assert_eq!(combo_for("kitty", &keys(&[])), DEFAULT_PASTE);
    }
}
