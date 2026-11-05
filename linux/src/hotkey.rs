use crate::session::Signal;

pub const KEY_F18: u16 = 188;
pub const EVENT_SIZE: usize = 24;

const EV_KEY: u16 = 1;
const MODIFIERS: [u16; 8] = [29, 42, 54, 56, 97, 100, 125, 126];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyState {
    Up,
    Down,
    Repeat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyEvent {
    pub code: u16,
    pub state: KeyState,
    pub at: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tracker {
    pub is_down: bool,
}

pub fn is_modifier(code: u16) -> bool {
    MODIFIERS.contains(&code)
}

pub fn step(tracker: Tracker, event: KeyEvent, hotkey: u16) -> (Tracker, Option<Signal>) {
    if event.code == hotkey {
        return on_hotkey(tracker, event.state);
    }
    if interrupts(tracker, event, hotkey) {
        return (Tracker::default(), Some(Signal::Interrupted));
    }
    (tracker, None)
}

fn on_hotkey(tracker: Tracker, state: KeyState) -> (Tracker, Option<Signal>) {
    let down = match state {
        KeyState::Repeat => return (tracker, None),
        KeyState::Down => true,
        KeyState::Up => false,
    };
    if down == tracker.is_down {
        return (tracker, None);
    }
    (Tracker { is_down: down }, Some(if down { Signal::Pressed } else { Signal::Released }))
}

fn interrupts(tracker: Tracker, event: KeyEvent, hotkey: u16) -> bool {
    if !is_modifier(hotkey) {
        return false;
    }
    tracker.is_down && event.state == KeyState::Down
}

pub fn decode_events(bytes: &[u8]) -> Vec<KeyEvent> {
    bytes.as_chunks::<EVENT_SIZE>().0.iter().filter_map(decode_event).collect()
}

fn decode_event(record: &[u8; EVENT_SIZE]) -> Option<KeyEvent> {
    let word = |at: usize| u16::from_le_bytes([record[at], record[at + 1]]);
    let long = |at: usize| i64::from_le_bytes(std::array::from_fn(|byte| record[at + byte]));
    if word(16) != EV_KEY {
        return None;
    }
    let state = match i32::from_le_bytes(std::array::from_fn(|byte| record[20 + byte])) {
        0 => KeyState::Up,
        1 => KeyState::Down,
        2 => KeyState::Repeat,
        _ => return None,
    };
    Some(KeyEvent { code: word(18), state, at: long(0) as f64 + long(8) as f64 / 1_000_000.0 })
}

pub fn has_key(capabilities: &str, code: u16) -> bool {
    let words: Vec<&str> = capabilities.split_whitespace().rev().collect();
    words
        .get(usize::from(code / 64))
        .and_then(|word| u64::from_str_radix(word, 16).ok())
        .is_some_and(|bits| (bits >> (code % 64)) & 1 == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT_META: u16 = 126;

    fn key(code: u16, state: KeyState) -> KeyEvent {
        KeyEvent { code, state, at: 0.0 }
    }

    fn signals(events: &[KeyEvent], hotkey: u16) -> Vec<Signal> {
        events
            .iter()
            .fold((Tracker::default(), vec![]), |(tracker, found), event| {
                let (next, signal) = step(tracker, *event, hotkey);
                (next, found.into_iter().chain(signal).collect())
            })
            .1
    }

    fn record(seconds: i64, micros: i64, kind: u16, code: u16, value: i32) -> Vec<u8> {
        [
            seconds.to_le_bytes().to_vec(),
            micros.to_le_bytes().to_vec(),
            kind.to_le_bytes().to_vec(),
            code.to_le_bytes().to_vec(),
            value.to_le_bytes().to_vec(),
        ]
        .concat()
    }

    #[test]
    fn press_and_release_give_signals() {
        let events = [key(KEY_F18, KeyState::Down), key(KEY_F18, KeyState::Up)];
        assert_eq!(signals(&events, KEY_F18), [Signal::Pressed, Signal::Released]);
    }

    #[test]
    fn repeats_and_a_second_down_trigger_once() {
        let events = [
            key(KEY_F18, KeyState::Down),
            key(KEY_F18, KeyState::Repeat),
            key(KEY_F18, KeyState::Down),
            key(KEY_F18, KeyState::Up),
        ];
        assert_eq!(signals(&events, KEY_F18), [Signal::Pressed, Signal::Released]);
    }

    #[test]
    fn a_stray_up_before_any_down_is_ignored() {
        let events = [key(KEY_F18, KeyState::Up), key(KEY_F18, KeyState::Down), key(KEY_F18, KeyState::Up)];
        assert_eq!(signals(&events, KEY_F18), [Signal::Pressed, Signal::Released]);
    }

    #[test]
    fn other_keys_do_not_interrupt_a_plain_hotkey() {
        let events = [key(KEY_F18, KeyState::Down), key(30, KeyState::Down), key(KEY_F18, KeyState::Up)];
        assert_eq!(signals(&events, KEY_F18), [Signal::Pressed, Signal::Released]);
    }

    #[test]
    fn a_modifier_hotkey_used_in_a_shortcut_is_an_interruption() {
        let events = [key(RIGHT_META, KeyState::Down), key(46, KeyState::Down), key(RIGHT_META, KeyState::Up)];
        assert_eq!(signals(&events, RIGHT_META), [Signal::Pressed, Signal::Interrupted]);
    }

    #[test]
    fn twenty_fast_taps_give_twenty_pairs() {
        let events: Vec<KeyEvent> =
            (0..20).flat_map(|_| [key(KEY_F18, KeyState::Down), key(KEY_F18, KeyState::Up)]).collect();
        assert_eq!(signals(&events, KEY_F18).len(), 40);
    }

    #[test]
    fn decodes_key_records_and_skips_others() {
        let bytes =
            [record(5, 250_000, EV_KEY, KEY_F18, 1), record(5, 250_001, 0, 0, 0), record(6, 0, EV_KEY, KEY_F18, 0)]
                .concat();
        assert_eq!(
            decode_events(&bytes),
            [
                KeyEvent { code: KEY_F18, state: KeyState::Down, at: 5.25 },
                KeyEvent { code: KEY_F18, state: KeyState::Up, at: 6.0 }
            ]
        );
    }

    #[test]
    fn a_partial_record_is_ignored() {
        assert!(decode_events(&record(1, 0, EV_KEY, KEY_F18, 1)[..20]).is_empty());
    }

    #[test]
    fn capabilities_bitmap_finds_keys_across_words() {
        let capabilities = "1000000000000000 0 0";
        assert!(has_key(capabilities, KEY_F18));
        assert!(!has_key(capabilities, 30));
        assert!(has_key("40000000", 30));
        assert!(!has_key("", KEY_F18));
    }
}
