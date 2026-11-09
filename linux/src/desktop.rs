use crate::herdr;
use crate::pin::Target;
use crate::windows::{self, Window};
use evdev::KeyCode;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::Duration;
pub use sth_voice::typing::Keyboard;

pub const DEFAULT_APP: &str = "default";
pub const DEFAULT_PASTE: &str = "shift+Insert";

const SETTLE: Duration = Duration::from_millis(60);
const HERDR_APP: &str = "herdr-app";

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

#[derive(Clone, Debug, PartialEq)]
pub enum Destination {
    Cursor { enter: bool },
    Pinned(Target),
}

pub trait Desktop: Send {
    fn deliver(&self, text: &str, destination: Destination) -> Result<(), String>;
    fn play(&self, sound: Sound);
}

pub struct Wayland {
    pub home: PathBuf,
    pub sounds: PathBuf,
    pub delivery: Delivery,
    pub paste_keys: BTreeMap<String, String>,
    pub keyboard: Keyboard,
}

impl Desktop for Wayland {
    fn deliver(&self, text: &str, destination: Destination) -> Result<(), String> {
        match destination {
            Destination::Cursor { enter } => self.at_cursor(text, enter),
            Destination::Pinned(Target::Herdr) => self.to_herdr(text),
            Destination::Pinned(Target::Window { address, .. }) => self.to_window(text, &address),
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Route {
    HerdrServer,
    HerdrApp,
    Keys,
}

pub fn route(window: &Window, runs_herdr: bool) -> Route {
    if window.class == HERDR_APP {
        return Route::HerdrApp;
    }
    if runs_herdr {
        return Route::HerdrServer;
    }
    Route::Keys
}

pub fn last_used_herdr(windows: &[Window], runs_herdr: impl Fn(&Window) -> bool) -> Route {
    windows
        .iter()
        .filter(|window| window.class == HERDR_APP || runs_herdr(window))
        .min_by_key(|window| window.focus_history)
        .map_or(Route::HerdrServer, |window| route(window, runs_herdr(window)))
}

pub fn shortcut_of(combo: &str) -> Result<(String, u16), String> {
    let codes = key_codes(combo)?;
    let key = codes.last().map(|code| code.code()).ok_or_else(|| format!("\"{combo}\" names no key"))?;
    let parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let mods: Vec<String> =
        parts.split_last().map_or(vec![], |(_, mods)| mods.iter().map(|m| m.to_uppercase()).collect());
    Ok((mods.join(" "), key))
}

pub fn blocked_by_x11(target: &Window, front: Option<&Window>) -> bool {
    target.xwayland && front.is_some_and(|front| front.xwayland && front.address != target.address)
}

impl Wayland {
    fn at_cursor(&self, text: &str, enter: bool) -> Result<(), String> {
        match self.delivery {
            Delivery::Paste => self.paste(text)?,
            Delivery::Type => wtype(&["--", text])?,
        }
        if !enter {
            return Ok(());
        }
        self.keyboard.press(&[KeyCode::KEY_ENTER])
    }

    fn paste(&self, text: &str) -> Result<(), String> {
        copy(text)?;
        let class = windows::active().map(|window| window.class).unwrap_or_default();
        self.keyboard.press(&key_codes(combo_for(&class, &self.paste_keys))?)
    }

    fn send_by(&self, route: Route, text: &str, window: Option<&Window>) -> Result<(), String> {
        match (route, window) {
            (Route::HerdrServer, _) => herdr::send_to_server(&self.home, text),
            (Route::HerdrApp, _) => herdr::send_to_app(&self.home, text),
            (Route::Keys, Some(window)) => self.keys_into(window, text),
            (Route::Keys, None) => Err("No window is pinned".into()),
        }
    }

    fn to_herdr(&self, text: &str) -> Result<(), String> {
        let processes = windows::parents();
        let route = last_used_herdr(&windows::all(), |window| windows::runs(&processes, window.pid, "herdr"));
        self.send_by(route, text, None)
    }

    fn to_window(&self, text: &str, address: &str) -> Result<(), String> {
        let window = windows::all()
            .into_iter()
            .find(|window| window.address == address)
            .ok_or("The pinned window is closed: pin another one")?;
        let runs_herdr = windows::runs(&windows::parents(), window.pid, "herdr");
        self.send_by(route(&window, runs_herdr), text, Some(&window))
    }

    fn keys_into(&self, window: &Window, text: &str) -> Result<(), String> {
        if blocked_by_x11(window, windows::active().as_ref()) {
            return Err(format!(
                "{} can't take text while another X11 app (the game) is in front, so nothing was sent",
                window.class
            ));
        }
        copy(text)?;
        let (mods, key) = shortcut_of(combo_for(&window.class, &self.paste_keys))?;
        windows::send_shortcut(&window.address, &mods, key)?;
        std::thread::sleep(SETTLE);
        windows::send_shortcut(&window.address, "", KeyCode::KEY_ENTER.code())
    }
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

    fn window(class: &str, history: i64, xwayland: bool) -> Window {
        Window {
            address: format!("0x{history}"),
            class: class.into(),
            focus_history: history,
            xwayland,
            ..Window::default()
        }
    }

    #[test]
    fn herdr_windows_route_to_their_own_server_and_others_get_keys() {
        assert_eq!(route(&window("herdr-app", 0, false), false), Route::HerdrApp);
        assert_eq!(route(&window("kitty", 0, false), true), Route::HerdrServer);
        assert_eq!(route(&window("Slack", 0, false), false), Route::Keys);
    }

    #[test]
    fn pinned_to_herdr_means_whichever_herdr_was_used_last() {
        let windows = [window("kitty", 3, false), window("herdr-app", 1, false), window("Slack", 0, false)];
        assert_eq!(last_used_herdr(&windows, |window| window.class == "kitty"), Route::HerdrApp);
        let kitty_last = [window("kitty", 1, false), window("herdr-app", 4, false)];
        assert_eq!(last_used_herdr(&kitty_last, |window| window.class == "kitty"), Route::HerdrServer);
        assert_eq!(last_used_herdr(&[], |_| false), Route::HerdrServer);
    }

    #[test]
    fn a_paste_combo_becomes_hyprland_mods_and_a_key_code() {
        assert_eq!(shortcut_of("shift+Insert").unwrap(), ("SHIFT".into(), KeyCode::KEY_INSERT.code()));
        assert_eq!(shortcut_of("ctrl+shift+v").unwrap(), ("CTRL SHIFT".into(), KeyCode::KEY_V.code()));
        assert_eq!(shortcut_of("v").unwrap(), (String::new(), KeyCode::KEY_V.code()));
    }

    #[test]
    fn an_x11_window_is_never_typed_into_while_another_x11_app_is_in_front() {
        let game = window("wow.exe", 0, true);
        assert!(blocked_by_x11(&window("discord", 2, true), Some(&game)));
        assert!(!blocked_by_x11(&window("Slack", 2, false), Some(&game)));
        assert!(!blocked_by_x11(&window("discord", 2, true), Some(&window("kitty", 0, false))));
    }
}
