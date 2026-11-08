use crate::broadcast::{Phase, Publish, Status};
use crate::desktop::{Desktop, Destination, Sound};
use crate::hotkey::{self, Tracker};
use crate::keyboard::Heard;
use crate::listening::{self, Heard as Listened, Tuning};
use crate::pin::{Pin, PinChange};
use crate::recorder::{Capture, Ear, Microphone, Stream};
use crate::session::{self, Action, Command, Session};
use crate::transcription::Transcriber;
use crate::wav;
use std::sync::Arc;
use std::sync::mpsc::Sender;

pub enum Input {
    Heard(Heard),
    Listened(Listened),
    Transcribed(u64, Result<String, String>),
    Pin(PinChange),
}

pub struct Ports {
    pub hotkey: u16,
    pub hotkey_label: String,
    pub microphone: Box<dyn Microphone>,
    pub ear: Box<dyn Ear>,
    pub tuning: Tuning,
    pub transcriber: Arc<dyn Transcriber>,
    pub desktop: Box<dyn Desktop>,
    pub publish: Box<dyn Publish>,
    pub remember_pin: Box<dyn Fn(&Pin)>,
    pub inbox: Sender<Input>,
}

#[derive(Default)]
pub struct Daemon {
    session: Session,
    tracker: Tracker,
    capture: Option<Box<dyn Capture>>,
    stream: Option<Box<dyn Stream>>,
    speaking: bool,
    devices: usize,
    message: String,
    shown: Option<Status>,
    pin: Pin,
}

impl Daemon {
    pub fn starting(pin: Pin) -> Daemon {
        Daemon { pin, ..Daemon::default() }
    }

    pub fn handle(self, input: Input, ports: &Ports) -> Daemon {
        let next = match input {
            Input::Heard(Heard::Key(event)) => self.on_key(event, ports),
            Input::Heard(Heard::Devices(devices)) => Daemon { devices, ..self },
            Input::Listened(heard) => self.on_listened(heard, ports),
            Input::Transcribed(id, Ok(text)) => self.dispatch(Action::Transcribed(id, text), ports),
            Input::Transcribed(id, Err(message)) => self.dispatch(Action::TranscriptionFailed(id, message), ports),
            Input::Pin(change) => self.on_pin(change, ports),
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

    fn on_listened(self, heard: Listened, ports: &Ports) -> Daemon {
        if self.stream.is_none() {
            return self;
        }
        match heard {
            Listened::Speaking => Daemon { speaking: true, ..self },
            Listened::Noise => Daemon { speaking: false, ..self },
            Listened::Piece(samples) => Daemon { speaking: false, ..self }.dispatch(Action::Heard(samples), ports),
        }
    }

    fn on_pin(self, change: PinChange, ports: &Ports) -> Daemon {
        let pin = change.apply(self.pin.clone());
        (ports.remember_pin)(&pin);
        Daemon { pin, message: String::new(), ..self }
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
            Command::StartStreaming => self.start_streaming(ports),
            Command::StopStreaming => self.stop_streaming(ports),
            Command::TranscribePiece(id, samples) => self.transcribe(id, samples, true, ports),
            Command::Paste { text, streamed } => self.deliver(&text, streamed, ports),
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

    fn start_streaming(self, ports: &Ports) -> Daemon {
        ports.desktop.play(Sound::Start);
        let inbox = ports.inbox.clone();
        let heard = Box::new(move |heard| inbox.send(Input::Listened(heard)).is_ok());
        match ports.ear.listen(ports.tuning, heard) {
            Ok(stream) => Daemon { stream: Some(stream), speaking: false, message: String::new(), ..self },
            Err(message) => Daemon { stream: None, message, ..self },
        }
    }

    fn stop_streaming(self, ports: &Ports) -> Daemon {
        ports.desktop.play(Sound::Stop);
        Daemon { stream: None, speaking: false, message: "Streaming off".into(), ..self }
    }

    fn stop_and_transcribe(self, id: u64, ports: &Ports) -> Daemon {
        ports.desktop.play(Sound::Stop);
        let (daemon, samples) = self.finish_capture();
        if wav::too_short(&samples) {
            let told = Daemon { message: "Too short, nothing sent".into(), ..daemon };
            return told.dispatch(Action::Transcribed(id, String::new()), ports);
        }
        daemon.transcribe(id, samples, false, ports)
    }

    fn transcribe(self, id: u64, samples: Vec<i16>, streamed: bool, ports: &Ports) -> Daemon {
        let audio = wav::wav(&wav::louder(&samples));
        let transcriber = ports.transcriber.clone();
        let inbox = ports.inbox.clone();
        std::thread::spawn(move || {
            let text = transcriber.transcribe(&audio).map(|text| keep_speech(text, streamed));
            inbox.send(Input::Transcribed(id, text))
        });
        let message = if streamed { self.message.clone() } else { "Transcribing…".into() };
        Daemon { message, ..self }
    }

    fn destination(&self, streamed: bool) -> Destination {
        match self.pin.target() {
            Some(target) => Destination::Pinned(target),
            None => Destination::Cursor { enter: streamed },
        }
    }

    fn deliver(self, text: &str, streamed: bool, ports: &Ports) -> Daemon {
        let message = ports.desktop.deliver(text, self.destination(streamed)).err().unwrap_or_default();
        Daemon { message, ..self }
    }

    pub fn status(&self, hotkey_label: &str) -> Status {
        let target = self.pin.target();
        let pinned_to = target.as_ref().map(|target| target.label()).unwrap_or_default();
        let pinned = self.pin.on;
        if self.devices == 0 {
            let message = format!("No keyboard sends {hotkey_label}");
            return Status { state: Phase::Unavailable, message, pinned, pinned_to, target };
        }
        let state = self.phase();
        let message = match (self.message.is_empty(), state.clone()) {
            (true, Phase::Streaming) => self.streaming_message(),
            (true, _) => ready(hotkey_label, &pinned_to),
            (false, _) => self.message.clone(),
        };
        Status { state, message, pinned, pinned_to, target }
    }

    fn streaming_message(&self) -> String {
        if self.speaking {
            return "Streaming: hearing you…".into();
        }
        if self.session.is_transcribing() {
            return "Streaming: sending…".into();
        }
        "Streaming: each pause sends what you said".into()
    }

    fn phase(&self) -> Phase {
        if self.session.is_streaming() {
            return Phase::Streaming;
        }
        if self.session.is_recording() {
            return Phase::Recording;
        }
        if self.session.is_transcribing() {
            return Phase::Transcribing;
        }
        Phase::Idle
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

fn keep_speech(text: String, streamed: bool) -> String {
    if streamed && listening::is_filler(&text) {
        return String::new();
    }
    text
}

fn ready(hotkey_label: &str, pinned_to: &str) -> String {
    if pinned_to.is_empty() {
        return format!("Ready: {hotkey_label} to dictate");
    }
    format!("Ready: {hotkey_label} to dictate into {pinned_to}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::{KEY_F18, KeyEvent, KeyState};
    use crate::pin::Target;
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

    struct FakeStream;

    impl Stream for FakeStream {}

    struct FakeEar(Log);

    impl Ear for FakeEar {
        fn listen(&self, _: Tuning, _: Box<dyn Fn(Listened) -> bool + Send>) -> Result<Box<dyn Stream>, String> {
            self.0.note("ear".into());
            Ok(Box::new(FakeStream))
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
        fn deliver(&self, text: &str, destination: Destination) -> Result<(), String> {
            let place = match destination {
                Destination::Cursor { enter: false } => "paste".to_string(),
                Destination::Cursor { enter: true } => "paste+enter".to_string(),
                Destination::Pinned(target) => format!("into {}", target.label()),
            };
            self.0.note(format!("{place}:{text}"));
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
        let remembered = shown.clone();
        let ports = Ports {
            hotkey: KEY_F18,
            hotkey_label: "Right ⌘".into(),
            microphone: Box::new(FakeMicrophone(microphone)),
            ear: Box::new(FakeEar(desktop.clone())),
            tuning: Tuning::new(1.0, 150.0),
            transcriber: Arc::new(FakeTranscriber(transcript)),
            desktop: Box::new(FakeDesk(desktop.clone(), pasted)),
            publish: Box::new(FakePublish(shown.clone())),
            remember_pin: Box::new(move |pin| remembered.note(format!("remembered:{}", pin.on))),
            inbox: sender,
        };
        Rig { ports, inbox, desktop, shown }
    }

    fn key(state: KeyState, at: f64) -> Input {
        Input::Heard(Heard::Key(KeyEvent { code: KEY_F18, state, at }))
    }

    fn press(daemon: Daemon, rig: &Rig, presses: &[(f64, f64)]) -> Daemon {
        presses
            .iter()
            .flat_map(|(down, up)| [key(KeyState::Down, *down), key(KeyState::Up, *up)])
            .fold(daemon, |daemon, input| daemon.handle(input, &rig.ports))
    }

    fn hold(daemon: Daemon, rig: &Rig) -> Daemon {
        press(daemon, rig, &[(1.0, 2.0)])
    }

    fn listening(rig: &Rig) -> Daemon {
        Daemon::default().handle(Input::Heard(Heard::Devices(1)), &rig.ports)
    }

    fn deliver_transcript(daemon: Daemon, rig: &Rig) -> Daemon {
        rig.inbox.recv().map(|input| daemon.handle(input, &rig.ports)).unwrap_or_default()
    }

    fn slack() -> Target {
        Target::Window { address: "0x1".into(), class: "Slack".into(), title: "general".into() }
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
        assert_eq!(daemon.status("Right ⌘").message, "Could not start pw-record");
        assert_eq!(daemon.status("Right ⌘").state, Phase::Recording);
    }

    #[test]
    fn a_paste_failure_is_shown() {
        let rig = rig(Ok(wav::silence(1.0)), Ok("hi".into()), Err("The pinned window is closed".into()));
        deliver_transcript(hold(listening(&rig), &rig), &rig);
        assert_eq!(rig.shown.entries().last().unwrap(), "Idle:The pinned window is closed");
    }

    #[test]
    fn pinned_text_goes_into_the_pinned_app() {
        let rig = rig(Ok(wav::silence(1.0)), Ok("hello agent".into()), Ok(()));
        let pinned = listening(&rig).handle(Input::Pin(PinChange::To(slack())), &rig.ports);
        deliver_transcript(hold(pinned, &rig), &rig);
        assert_eq!(rig.desktop.entries(), ["Start", "Stop", "into Slack:hello agent"]);
    }

    #[test]
    fn pin_on_with_nothing_chosen_goes_to_herdr_and_is_shown() {
        let rig = rig(Ok(vec![]), Ok("x".into()), Ok(()));
        let pinned = listening(&rig).handle(Input::Pin(PinChange::On), &rig.ports);
        pinned.handle(Input::Pin(PinChange::Off), &rig.ports);
        assert_eq!(
            rig.shown.entries(),
            [
                "Idle:Ready: Right ⌘ to dictate",
                "remembered:true",
                "Idle:Ready: Right ⌘ to dictate into Herdr",
                "remembered:false",
                "Idle:Ready: Right ⌘ to dictate"
            ]
        );
    }

    #[test]
    fn a_double_tap_streams_and_each_piece_is_sent_with_enter() {
        let rig = rig(Ok(wav::silence(1.0)), Ok("next step please".into()), Ok(()));
        let streaming = press(listening(&rig), &rig, &[(1.0, 1.05), (1.2, 1.25)]);
        assert_eq!(streaming.status("Right ⌘").state, Phase::Streaming);
        let speaking = streaming.handle(Input::Listened(Listened::Speaking), &rig.ports);
        assert_eq!(speaking.status("Right ⌘").message, "Streaming: hearing you…");
        let heard = speaking.handle(Input::Listened(Listened::Piece(wav::silence(1.0))), &rig.ports);
        let sent = deliver_transcript(heard, &rig);
        assert_eq!(rig.desktop.entries(), ["Start", "Start", "ear", "paste+enter:next step please"]);
        let stopped = press(sent, &rig, &[(5.0, 5.05), (5.2, 5.25)]);
        assert_eq!(rig.desktop.entries().last().unwrap(), "Stop");
        assert_eq!(stopped.status("Right ⌘").state, Phase::Idle);
    }

    #[test]
    fn streamed_filler_from_silence_is_not_sent() {
        let rig = rig(Ok(wav::silence(1.0)), Ok(" Thank you. ".into()), Ok(()));
        let streaming = press(listening(&rig), &rig, &[(1.0, 1.05), (1.2, 1.25)]);
        let heard = streaming.handle(Input::Listened(Listened::Piece(wav::silence(1.0))), &rig.ports);
        deliver_transcript(heard, &rig);
        assert_eq!(rig.desktop.entries(), ["Start", "Start", "ear"]);
    }

    #[test]
    fn without_a_keyboard_it_is_unavailable() {
        let rig = rig(Ok(vec![]), Ok("x".into()), Ok(()));
        Daemon::default().handle(Input::Heard(Heard::Devices(0)), &rig.ports);
        assert_eq!(rig.shown.entries(), ["Unavailable:No keyboard sends Right ⌘"]);
    }

    #[test]
    fn a_daemon_can_start_pinned() {
        let rig = rig(Ok(vec![]), Ok("x".into()), Ok(()));
        let pin = PinChange::To(slack()).apply(Pin::default());
        let daemon = Daemon::starting(pin).handle(Input::Heard(Heard::Devices(1)), &rig.ports);
        assert_eq!(daemon.status("Right ⌘").pinned_to, "Slack");
    }

    #[test]
    fn an_unchanged_status_is_not_sent_again() {
        let rig = rig(Ok(vec![]), Ok("x".into()), Ok(()));
        let daemon = listening(&rig);
        daemon.handle(Input::Heard(Heard::Devices(1)), &rig.ports);
        assert_eq!(rig.shown.entries(), ["Idle:Ready: Right ⌘ to dictate"]);
    }
}
