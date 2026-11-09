use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, KeyCode, KeyEvent};
use std::sync::Mutex;
use std::time::Duration;

const KEY_GAP: Duration = Duration::from_millis(12);
const KEYBOARD_KEYS: std::ops::RangeInclusive<u16> = 1..=248;

const UNSHIFTED: &str = "abcdefghijklmnopqrstuvwxyz1234567890 -=[]\\;',./`";
const SHIFTED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*() _+{}|:\"<>?~";
const KEYS: [KeyCode; 48] = [
    KeyCode::KEY_A,
    KeyCode::KEY_B,
    KeyCode::KEY_C,
    KeyCode::KEY_D,
    KeyCode::KEY_E,
    KeyCode::KEY_F,
    KeyCode::KEY_G,
    KeyCode::KEY_H,
    KeyCode::KEY_I,
    KeyCode::KEY_J,
    KeyCode::KEY_K,
    KeyCode::KEY_L,
    KeyCode::KEY_M,
    KeyCode::KEY_N,
    KeyCode::KEY_O,
    KeyCode::KEY_P,
    KeyCode::KEY_Q,
    KeyCode::KEY_R,
    KeyCode::KEY_S,
    KeyCode::KEY_T,
    KeyCode::KEY_U,
    KeyCode::KEY_V,
    KeyCode::KEY_W,
    KeyCode::KEY_X,
    KeyCode::KEY_Y,
    KeyCode::KEY_Z,
    KeyCode::KEY_1,
    KeyCode::KEY_2,
    KeyCode::KEY_3,
    KeyCode::KEY_4,
    KeyCode::KEY_5,
    KeyCode::KEY_6,
    KeyCode::KEY_7,
    KeyCode::KEY_8,
    KeyCode::KEY_9,
    KeyCode::KEY_0,
    KeyCode::KEY_SPACE,
    KeyCode::KEY_MINUS,
    KeyCode::KEY_EQUAL,
    KeyCode::KEY_LEFTBRACE,
    KeyCode::KEY_RIGHTBRACE,
    KeyCode::KEY_BACKSLASH,
    KeyCode::KEY_SEMICOLON,
    KeyCode::KEY_APOSTROPHE,
    KeyCode::KEY_COMMA,
    KeyCode::KEY_DOT,
    KeyCode::KEY_SLASH,
    KeyCode::KEY_GRAVE,
];

fn plain(character: char) -> char {
    match character {
        '‘' | '’' => '\'',
        '“' | '”' => '"',
        '–' | '—' => '-',
        '…' => '.',
        other => other,
    }
}

pub fn keystroke(character: char) -> Option<Vec<KeyCode>> {
    let character = plain(character);
    let unshifted = UNSHIFTED.chars().position(|known| known == character).map(|at| vec![KEYS[at]]);
    let shifted =
        || SHIFTED.chars().position(|known| known == character).map(|at| vec![KeyCode::KEY_LEFTSHIFT, KEYS[at]]);
    unshifted.or_else(shifted)
}

pub fn keystrokes(text: &str) -> Vec<Vec<KeyCode>> {
    text.chars().filter_map(keystroke).collect()
}

pub struct Keyboard(Mutex<VirtualDevice>);

impl Keyboard {
    pub fn create(name: &str) -> Result<Keyboard, String> {
        let keys: AttributeSet<KeyCode> = KEYBOARD_KEYS.map(KeyCode::new).collect();
        VirtualDevice::builder()
            .and_then(|builder| builder.name(name).with_keys(&keys))
            .and_then(|builder| builder.build())
            .map(|device| Keyboard(Mutex::new(device)))
            .map_err(|error| format!("Could not create a virtual keyboard on /dev/uinput: {error}"))
    }

    pub fn press(&self, codes: &[KeyCode]) -> Result<(), String> {
        let mut device = self.0.lock().map_err(|_| "The virtual keyboard is unusable".to_string())?;
        let downs = codes.iter().map(|code| (*code, 1));
        let ups = codes.iter().rev().map(|code| (*code, 0));
        downs.chain(ups).try_for_each(|(code, value)| {
            std::thread::sleep(KEY_GAP);
            device.emit(&[*KeyEvent::new(code, value)]).map_err(|error| format!("Could not press keys: {error}"))
        })
    }

    pub fn type_text(&self, text: &str) -> Result<(), String> {
        keystrokes(text).iter().try_for_each(|codes| self.press(codes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_punctuation_map_to_us_keys() {
        assert_eq!(keystroke('a'), Some(vec![KeyCode::KEY_A]));
        assert_eq!(keystroke('A'), Some(vec![KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_A]));
        assert_eq!(keystroke('/'), Some(vec![KeyCode::KEY_SLASH]));
        assert_eq!(keystroke('?'), Some(vec![KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_SLASH]));
        assert_eq!(keystroke(' '), Some(vec![KeyCode::KEY_SPACE]));
    }

    #[test]
    fn curly_quotes_become_plain_and_unknown_characters_are_skipped() {
        assert_eq!(keystroke('’'), Some(vec![KeyCode::KEY_APOSTROPHE]));
        assert_eq!(keystroke('é'), None);
        assert_eq!(keystrokes("Hi é!").len(), 4);
    }

    #[test]
    fn both_tables_line_up_with_the_keys() {
        assert_eq!(UNSHIFTED.chars().count(), KEYS.len());
        assert_eq!(SHIFTED.chars().count(), KEYS.len());
    }
}
