use crate::session::Signal;
pub use sth_voice::keys::{KeyEvent, KeyState};

pub const KEY_F18: u16 = 188;

const MODIFIERS: [u16; 8] = [29, 42, 54, 56, 97, 100, 125, 126];

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
}
