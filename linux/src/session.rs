use std::collections::{BTreeMap, BTreeSet};

pub const HOLD_THRESHOLD_SECONDS: f64 = 0.4;
pub const DOUBLE_TAP_SECONDS: f64 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Signal {
    Pressed,
    Released,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyPhase {
    HeldSinceStart(f64),
    Toggled(f64),
    StopPressDown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recording {
    pub id: u64,
    pub key: KeyPhase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Streaming {
    pub last_press: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub recording: Option<Recording>,
    pub streaming: Option<Streaming>,
    pub next_id: u64,
    pub next_to_paste: u64,
    pub finished: BTreeMap<u64, String>,
    pub streamed: BTreeSet<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Hotkey(Signal, f64),
    Heard(Vec<i16>),
    Transcribed(u64, String),
    TranscriptionFailed(u64, String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    StartCapture(u64),
    StopCaptureAndTranscribe(u64),
    DiscardCapture(u64),
    StartStreaming,
    StopStreaming,
    TranscribePiece(u64, Vec<i16>),
    Paste { text: String, streamed: bool },
    Report(String),
}

impl Session {
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }

    pub fn is_transcribing(&self) -> bool {
        self.next_to_paste + u64::from(self.is_recording()) < self.next_id
    }

    fn with_recording(self, recording: Option<Recording>) -> Session {
        Session { recording, ..self }
    }
}

pub fn reduce(session: Session, action: Action) -> (Session, Vec<Command>) {
    match action {
        Action::Hotkey(signal, at) => on_hotkey(session, signal, at),
        Action::Heard(samples) => on_heard(session, samples),
        Action::Transcribed(id, text) => flush(session, id, text.trim().to_string(), vec![]),
        Action::TranscriptionFailed(id, message) => flush(session, id, String::new(), vec![Command::Report(message)]),
    }
}

fn on_hotkey(session: Session, signal: Signal, at: f64) -> (Session, Vec<Command>) {
    if let Some(streaming) = session.streaming {
        return on_streaming_key(session, streaming, signal, at);
    }
    match session.recording {
        Some(recording) => on_key(session, recording, signal, at),
        None => on_idle_key(session, signal, at),
    }
}

fn on_streaming_key(session: Session, streaming: Streaming, signal: Signal, at: f64) -> (Session, Vec<Command>) {
    if signal != Signal::Pressed {
        return (session, vec![]);
    }
    if streaming.last_press.is_some_and(|last| at - last < DOUBLE_TAP_SECONDS) {
        return (Session { streaming: None, ..session }, vec![Command::StopStreaming]);
    }
    (Session { streaming: Some(Streaming { last_press: Some(at) }), ..session }, vec![])
}

fn on_heard(session: Session, samples: Vec<i16>) -> (Session, Vec<Command>) {
    if !session.is_streaming() {
        return (session, vec![]);
    }
    let id = session.next_id;
    let streamed = session.streamed.iter().copied().chain([id]).collect();
    (Session { next_id: id + 1, streamed, ..session }, vec![Command::TranscribePiece(id, samples)])
}

fn on_idle_key(session: Session, signal: Signal, at: f64) -> (Session, Vec<Command>) {
    if signal != Signal::Pressed {
        return (session, vec![]);
    }
    let id = session.next_id;
    let recording = Recording { id, key: KeyPhase::HeldSinceStart(at) };
    (Session { recording: Some(recording), next_id: id + 1, ..session }, vec![Command::StartCapture(id)])
}

fn on_key(session: Session, recording: Recording, signal: Signal, at: f64) -> (Session, Vec<Command>) {
    match (recording.key, signal) {
        (KeyPhase::HeldSinceStart(start), Signal::Released) => on_first_release(session, recording.id, start, at),
        (KeyPhase::HeldSinceStart(_), Signal::Interrupted) => discard(session, recording.id),
        (KeyPhase::Toggled(start), Signal::Pressed) => on_second_press(session, recording.id, start, at),
        (KeyPhase::StopPressDown, Signal::Released) => stop(session, recording.id),
        (KeyPhase::StopPressDown, Signal::Interrupted) => {
            key_phase(session, recording.id, KeyPhase::Toggled(f64::NEG_INFINITY))
        }
        _ => (session, vec![]),
    }
}

fn on_first_release(session: Session, id: u64, start: f64, at: f64) -> (Session, Vec<Command>) {
    if at - start >= HOLD_THRESHOLD_SECONDS {
        return stop(session, id);
    }
    key_phase(session, id, KeyPhase::Toggled(start))
}

fn on_second_press(session: Session, id: u64, start: f64, at: f64) -> (Session, Vec<Command>) {
    if at - start < DOUBLE_TAP_SECONDS {
        return start_streaming(session, id);
    }
    key_phase(session, id, KeyPhase::StopPressDown)
}

fn start_streaming(session: Session, id: u64) -> (Session, Vec<Command>) {
    let (discarded, commands) = discard(session, id);
    let streaming = Session { streaming: Some(Streaming::default()), ..discarded };
    (streaming, commands.into_iter().chain([Command::StartStreaming]).collect())
}

fn key_phase(session: Session, id: u64, key: KeyPhase) -> (Session, Vec<Command>) {
    (session.with_recording(Some(Recording { id, key })), vec![])
}

fn stop(session: Session, id: u64) -> (Session, Vec<Command>) {
    (session.with_recording(None), vec![Command::StopCaptureAndTranscribe(id)])
}

fn discard(session: Session, id: u64) -> (Session, Vec<Command>) {
    flush(session.with_recording(None), id, String::new(), vec![Command::DiscardCapture(id)])
}

fn flush(session: Session, id: u64, text: String, extra: Vec<Command>) -> (Session, Vec<Command>) {
    let finished: BTreeMap<u64, String> = session.finished.into_iter().chain([(id, text)]).collect();
    let ready = (session.next_to_paste..).take_while(|candidate| finished.contains_key(candidate)).count() as u64;
    let up_to = session.next_to_paste + ready;
    let (pasted, remaining): (BTreeMap<u64, String>, BTreeMap<u64, String>) =
        finished.into_iter().partition(|(finished_id, _)| *finished_id < up_to);
    let pastes: Vec<Command> = pasted
        .into_iter()
        .filter(|(_, text)| !text.is_empty())
        .map(|(id, text)| Command::Paste { text, streamed: session.streamed.contains(&id) })
        .collect();
    let streamed = session.streamed.into_iter().filter(|streamed_id| *streamed_id >= up_to).collect();
    let next = Session { next_to_paste: up_to, finished: remaining, streamed, ..session };
    (next, extra.into_iter().chain(pastes).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(start: Session, actions: Vec<Action>) -> (Session, Vec<Command>) {
        actions.into_iter().fold((start, vec![]), |(session, commands), action| {
            let (next, more) = reduce(session, action);
            (next, commands.into_iter().chain(more).collect())
        })
    }

    fn taps(times: &[f64], width: f64) -> Vec<Action> {
        times
            .iter()
            .flat_map(|at| [Action::Hotkey(Signal::Pressed, *at), Action::Hotkey(Signal::Released, at + width)])
            .collect()
    }

    fn kinds(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .map(|command| match command {
                Command::StartCapture(id) => format!("start{id}"),
                Command::StopCaptureAndTranscribe(id) => format!("stop{id}"),
                Command::DiscardCapture(id) => format!("discard{id}"),
                Command::StartStreaming => "stream-on".into(),
                Command::StopStreaming => "stream-off".into(),
                Command::TranscribePiece(id, _) => format!("piece{id}"),
                Command::Paste { text, streamed: false } => format!("paste:{text}"),
                Command::Paste { text, streamed: true } => format!("send:{text}"),
                Command::Report(message) => format!("report:{message}"),
            })
            .collect()
    }

    fn streaming() -> Session {
        run(Session::default(), taps(&[0.0, 0.2], 0.05)).0
    }

    #[test]
    fn tap_starts_and_second_tap_stops() {
        let (session, commands) = run(Session::default(), taps(&[0.0, 1.0], 0.05));
        assert_eq!(kinds(&commands), ["start0", "stop0"]);
        assert!(!session.is_recording());
    }

    #[test]
    fn holding_then_releasing_stops() {
        let (_, commands) = run(Session::default(), taps(&[0.0], 1.0));
        assert_eq!(kinds(&commands), ["start0", "stop0"]);
    }

    #[test]
    fn a_shortcut_mid_dictation_keeps_recording() {
        let actions = [taps(&[0.0], 0.05), vec![Action::Hotkey(Signal::Interrupted, 0.5)]].concat();
        let (session, commands) = run(Session::default(), actions);
        assert_eq!(kinds(&commands), ["start0"]);
        assert!(session.is_recording());
    }

    #[test]
    fn a_shortcut_on_the_first_press_discards() {
        let actions = vec![Action::Hotkey(Signal::Pressed, 0.0), Action::Hotkey(Signal::Interrupted, 0.1)];
        let (session, commands) = run(Session::default(), actions);
        assert_eq!(kinds(&commands), ["start0", "discard0"]);
        assert!(!session.is_recording());
    }

    #[test]
    fn an_interruption_during_the_stop_press_keeps_recording() {
        let actions = [
            taps(&[0.0], 0.05),
            vec![Action::Hotkey(Signal::Pressed, 1.0), Action::Hotkey(Signal::Interrupted, 1.1)],
            vec![Action::Hotkey(Signal::Released, 1.2)],
        ]
        .concat();
        let (session, commands) = run(Session::default(), actions);
        assert_eq!(kinds(&commands), ["start0"]);
        assert!(session.is_recording());
    }

    #[test]
    fn a_double_tap_turns_streaming_on_and_drops_the_tap_recording() {
        let (session, commands) = run(Session::default(), taps(&[0.0, 0.2], 0.05));
        assert_eq!(kinds(&commands), ["start0", "discard0", "stream-on"]);
        assert!(session.is_streaming());
        assert!(!session.is_recording());
    }

    #[test]
    fn a_second_double_tap_turns_streaming_off() {
        let (session, commands) = run(streaming(), taps(&[5.0, 5.2], 0.05));
        assert_eq!(kinds(&commands), ["stream-off"]);
        assert!(!session.is_streaming());
    }

    #[test]
    fn a_single_tap_while_streaming_does_nothing() {
        let (session, commands) = run(streaming(), taps(&[5.0, 6.0], 0.05));
        assert!(commands.is_empty());
        assert!(session.is_streaming());
    }

    #[test]
    fn speech_heard_while_streaming_is_transcribed_and_sent_in_order() {
        let actions = vec![
            Action::Heard(vec![1]),
            Action::Heard(vec![2]),
            Action::Transcribed(2, "second".into()),
            Action::Transcribed(1, "first".into()),
        ];
        let (session, commands) = run(streaming(), actions);
        assert_eq!(kinds(&commands), ["piece1", "piece2", "send:first", "send:second"]);
        assert!(session.streamed.is_empty());
    }

    #[test]
    fn speech_heard_after_streaming_stops_is_dropped() {
        assert!(run(Session::default(), vec![Action::Heard(vec![1])]).1.is_empty());
    }

    #[test]
    fn taps_further_apart_than_a_double_tap_are_separate_recordings() {
        let (session, commands) = run(Session::default(), taps(&[0.0, 0.5, 1.0, 1.5], 0.01));
        assert_eq!(kinds(&commands), ["start0", "stop0", "start1", "stop1"]);
        assert!(!session.is_recording());
    }

    #[test]
    fn a_tap_right_after_stopping_starts_a_new_recording() {
        let (session, commands) = run(Session::default(), taps(&[0.0, 1.0, 1.02], 0.001));
        assert_eq!(kinds(&commands), ["start0", "stop0", "start1"]);
        assert!(session.is_recording());
        assert!(session.is_transcribing());
    }

    #[test]
    fn release_and_next_press_in_the_same_instant_both_register() {
        let actions = vec![
            Action::Hotkey(Signal::Pressed, 0.0),
            Action::Hotkey(Signal::Released, 1.0),
            Action::Hotkey(Signal::Pressed, 1.0),
        ];
        assert_eq!(kinds(&run(Session::default(), actions).1), ["start0", "stop0", "start1"]);
    }

    #[test]
    fn a_stray_release_while_idle_does_nothing() {
        assert!(run(Session::default(), vec![Action::Hotkey(Signal::Released, 0.0)]).1.is_empty());
    }

    #[test]
    fn results_paste_in_the_order_spoken_and_trimmed() {
        let actions = [
            taps(&[0.0, 1.0, 2.0, 3.0], 0.01),
            vec![Action::Transcribed(1, " second ".into()), Action::Transcribed(0, "first\n".into())],
        ]
        .concat();
        let (session, commands) = run(Session::default(), actions);
        assert_eq!(kinds(&commands)[4..], ["paste:first", "paste:second"]);
        assert!(!session.is_transcribing());
        assert!(session.finished.is_empty());
    }

    #[test]
    fn a_failure_is_reported_and_later_results_still_paste() {
        let actions = [
            taps(&[0.0, 1.0, 2.0, 3.0], 0.01),
            vec![Action::Transcribed(1, "second".into()), Action::TranscriptionFailed(0, "down".into())],
        ]
        .concat();
        assert_eq!(kinds(&run(Session::default(), actions).1)[4..], ["report:down", "paste:second"]);
    }

    #[test]
    fn empty_transcripts_paste_nothing() {
        let session = Session { next_id: 1, ..Session::default() };
        assert!(reduce(session, Action::Transcribed(0, "  ".into())).1.is_empty());
    }
}
