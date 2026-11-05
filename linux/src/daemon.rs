use crate::broadcast::{Phase, Publish, Status};
use crate::desktop::{Desktop, Sound};
use crate::hotkey::{self, Tracker};
use crate::keyboard::Heard;
use crate::recorder::{Capture, Microphone};
use crate::session::{self, Action, Command, Session};
use crate::transcription::Transcriber;
use crate::wav;
use std::sync::Arc;
use std::sync::mpsc::Sender;

pub enum Input {
    Heard(Heard),
    Transcribed(u64, Result<String, String>),
}

pub struct Ports {
    pub hotkey: u16,
    pub hotkey_label: String,
    pub microphone: Box<dyn Microphone>,
    pub transcriber: Arc<dyn Transcriber>,
    pub desktop: Box<dyn Desktop>,
    pub publish: Box<dyn Publish>,
    pub inbox: Sender<Input>,
}

#[derive(Default)]
pub struct Daemon {
    session: Session,
    tracker: Tracker,
    capture: Option<Box<dyn Capture>>,
    devices: usize,
    message: String,
    shown: Option<Status>,
}

impl Daemon {
    pub fn handle(self, input: Input, ports: &Ports) -> Daemon {
        let next = match input {
            Input::Heard(Heard::Key(event)) => self.on_key(event, ports),
            Input::Heard(Heard::Devices(devices)) => Daemon { devices, ..self },
            Input::Transcribed(id, Ok(text)) => self.dispatch(Action::Transcribed(id, text), ports),
            Input::Transcribed(id, Err(message)) => self.dispatch(Action::TranscriptionFailed(id, message), ports),
        };
        next.show(ports)
    }

    fn on_key(self, event: hotkey::KeyEvent, ports: &Ports) -> Daemon {
        let (tracker, signal) = hotkey::step(self.tracker, event, ports.hotkey);
        let tracked = Daemon { tracker, ..self };
        match signal {
            Some(signal) => tracked.dispatch(Action::Hotkey(signal, event.at), ports),
            None => tracked,
        }
    }

    fn dispatch(self, action: Action, ports: &Ports) -> Daemon {
        let (session, commands) = session::reduce(self.session, action);
        commands.into_iter().fold(Daemon { session, ..self }, |daemon, command| daemon.run(command, ports))
    }

    fn run(self, command: Command, ports: &Ports) -> Daemon {
        match command {
            Command::StartCapture(_) => self.start_capture(ports),
            Command::StopCaptureAndTranscribe(id) => self.stop_and_transcribe(id, ports),
            Command::DiscardCapture(_) => self.discard_capture(),
            Command::Paste(text) => self.paste(&text, ports),
            Command::Report(message) => Daemon { message, ..self },
        }
    }

    fn start_capture(self, ports: &Ports) -> Daemon {
        ports.desktop.play(Sound::Start);
        match ports.microphone.start() {
            Ok(capture) => Daemon { capture: Some(capture), message: "Listening…".into(), ..self },
            Err(message) => Daemon { capture: None, message, ..self },
        }
    }

    fn finish_capture(self) -> (Daemon, Vec<i16>) {
        let samples = self.capture.map(Capture::finish).unwrap_or_default();
        (Daemon { capture: None, ..self }, samples)
    }

    fn discard_capture(self) -> Daemon {
        let (daemon, _) = self.finish_capture();
        Daemon { message: "Discarded".into(), ..daemon }
    }

    fn stop_and_transcribe(self, id: u64, ports: &Ports) -> Daemon {
        ports.desktop.play(Sound::Stop);
        let (daemon, samples) = self.finish_capture();
        if wav::too_short(&samples) {
            let told = Daemon { message: "Too short, nothing sent".into(), ..daemon };
            return told.dispatch(Action::Transcribed(id, String::new()), ports);
        }
        let audio = wav::wav(&wav::louder(&samples));
        let transcriber = ports.transcriber.clone();
        let inbox = ports.inbox.clone();
        std::thread::spawn(move || inbox.send(Input::Transcribed(id, transcriber.transcribe(&audio))));
        Daemon { message: "Transcribing…".into(), ..daemon }
    }

    fn paste(self, text: &str, ports: &Ports) -> Daemon {
        let message = ports.desktop.paste(text).err().unwrap_or_default();
        Daemon { message, ..self }
    }

    pub fn status(&self, hotkey_label: &str) -> Status {
        if self.devices == 0 {
            return Status { state: Phase::Unavailable, message: format!("No keyboard sends {hotkey_label}") };
        }
        let state = phase(&self.session);
        let message =
            if self.message.is_empty() { format!("Ready: {hotkey_label} to dictate") } else { self.message.clone() };
        Status { state, message }
    }

    fn show(self, ports: &Ports) -> Daemon {
        let status = self.status(&ports.hotkey_label);
        if self.shown.as_ref() == Some(&status) {
            return self;
        }
        ports.publish.publish(&status);
        Daemon { shown: Some(status), ..self }
    }
}

fn phase(session: &Session) -> Phase {
    if session.is_recording() {
        return Phase::Recording;
    }
    if session.is_transcribing() {
        return Phase::Transcribing;
    }
    Phase::Idle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::{KEY_F18, KeyEvent, KeyState};
    use std::sync::Mutex;
    use std::sync::mpsc::{Receiver, channel};

    #[derive(Clone, Default)]
    struct Log(Arc<Mutex<Vec<String>>>);

    impl Log {
        fn note(&self, entry: String) {
            if let Ok(mut entries) = self.0.lock() {
                entries.push(entry);
            }
        }

        fn entries(&self) -> Vec<String> {
            self.0.lock().map(|entries| entries.clone()).unwrap_or_default()
        }
    }

    struct FakeCapture(Vec<i16>);

    impl Capture for FakeCapture {
        fn finish(self: Box<Self>) -> Vec<i16> {
            self.0
        }
    }

    struct FakeMicrophone(Result<Vec<i16>, String>);

    impl Microphone for FakeMicrophone {
        fn start(&self) -> Result<Box<dyn Capture>, String> {
            self.0.clone().map(|samples| Box::new(FakeCapture(samples)) as Box<dyn Capture>)
        }
    }

    struct FakeTranscriber(Result<String, String>);

    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, audio: &[u8]) -> Result<String, String> {
            assert_eq!(&audio[..4], b"RIFF");
            self.0.clone()
        }
    }

    struct FakeDesk(Log, Result<(), String>);

    impl Desktop for FakeDesk {
        fn paste(&self, text: &str) -> Result<(), String> {
            self.0.note(format!("paste:{text}"));
            self.1.clone()
        }

        fn play(&self, sound: Sound) {
            self.0.note(format!("{sound:?}"));
        }
    }

    struct FakePublish(Log);

    impl Publish for FakePublish {
        fn publish(&self, status: &Status) {
            self.0.note(format!("{:?}:{}", status.state, status.message));
        }
    }

    struct Rig {
        ports: Ports,
        inbox: Receiver<Input>,
        desktop: Log,
        shown: Log,
    }

    fn rig(
        microphone: Result<Vec<i16>, String>,
        transcript: Result<String, String>,
        pasted: Result<(), String>,
    ) -> Rig {
        let (sender, inbox) = channel();
        let (desktop, shown) = (Log::default(), Log::default());
        let ports = Ports {
            hotkey: KEY_F18,
            hotkey_label: "Right ⌘".into(),
            microphone: Box::new(FakeMicrophone(microphone)),
            transcriber: Arc::new(FakeTranscriber(transcript)),
            desktop: Box::new(FakeDesk(desktop.clone(), pasted)),
            publish: Box::new(FakePublish(shown.clone())),
            inbox: sender,
        };
        Rig { ports, inbox, desktop, shown }
    }

    fn key(state: KeyState, at: f64) -> Input {
        Input::Heard(Heard::Key(KeyEvent { code: KEY_F18, state, at }))
    }

    fn hold(daemon: Daemon, rig: &Rig) -> Daemon {
        [key(KeyState::Down, 1.0), key(KeyState::Up, 2.0)]
            .into_iter()
            .fold(daemon, |daemon, input| daemon.handle(input, &rig.ports))
    }

    fn listening(rig: &Rig) -> Daemon {
        Daemon::default().handle(Input::Heard(Heard::Devices(1)), &rig.ports)
    }

    fn deliver_transcript(daemon: Daemon, rig: &Rig) -> Daemon {
        rig.inbox.recv().map(|input| daemon.handle(input, &rig.ports)).unwrap_or_default()
    }

    #[test]
    fn holding_the_key_records_transcribes_and_pastes() {
        let rig = rig(Ok(wav::silence(1.0)), Ok("hello there".into()), Ok(()));
        let daemon = deliver_transcript(hold(listening(&rig), &rig), &rig);
        assert_eq!(rig.desktop.entries(), ["Start", "Stop", "paste:hello there"]);
        assert_eq!(
            rig.shown.entries(),
            [
                "Idle:Ready: Right ⌘ to dictate",
                "Recording:Listening…",
                "Transcribing:Transcribing…",
                "Idle:Ready: Right ⌘ to dictate"
            ]
        );
        assert_eq!(daemon.status("Right ⌘").state, Phase::Idle);
    }

    #[test]
    fn a_short_recording_sends_nothing() {
        let rig = rig(Ok(wav::silence(0.1)), Ok("never".into()), Ok(()));
        hold(listening(&rig), &rig);
        assert_eq!(rig.desktop.entries(), ["Start", "Stop"]);
        assert!(rig.inbox.try_recv().is_err());
        assert_eq!(rig.shown.entries().last().unwrap(), "Idle:Too short, nothing sent");
    }

    #[test]
    fn a_failed_transcription_is_shown() {
        let rig = rig(Ok(wav::silence(1.0)), Err("Server answered 500: boom".into()), Ok(()));
        deliver_transcript(hold(listening(&rig), &rig), &rig);
        assert_eq!(rig.shown.entries().last().unwrap(), "Idle:Server answered 500: boom");
    }

    #[test]
    fn a_microphone_that_will_not_start_is_shown() {
        let rig = rig(Err("Could not start pw-record".into()), Ok("x".into()), Ok(()));
        let daemon = listening(&rig).handle(key(KeyState::Down, 1.0), &rig.ports);
        assert_eq!(
            daemon.status("Right ⌘"),
            Status { state: Phase::Recording, message: "Could not start pw-record".into() }
        );
    }

    #[test]
    fn a_paste_failure_is_shown() {
        let rig = rig(Ok(wav::silence(1.0)), Ok("hi".into()), Err("wtype could not press Shift+Insert".into()));
        deliver_transcript(hold(listening(&rig), &rig), &rig);
        assert_eq!(rig.shown.entries().last().unwrap(), "Idle:wtype could not press Shift+Insert");
    }

    #[test]
    fn without_a_keyboard_it_is_unavailable() {
        let rig = rig(Ok(vec![]), Ok("x".into()), Ok(()));
        Daemon::default().handle(Input::Heard(Heard::Devices(0)), &rig.ports);
        assert_eq!(rig.shown.entries(), ["Unavailable:No keyboard sends Right ⌘"]);
    }

    #[test]
    fn an_unchanged_status_is_not_sent_again() {
        let rig = rig(Ok(vec![]), Ok("x".into()), Ok(()));
        let daemon = listening(&rig);
        daemon.handle(Input::Heard(Heard::Devices(1)), &rig.ports);
        assert_eq!(rig.shown.entries(), ["Idle:Ready: Right ⌘ to dictate"]);
    }
}
