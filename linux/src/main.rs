mod broadcast;
mod daemon;
mod desktop;
mod herdr;
mod hotkey;
mod pin;
mod session;
mod settings;
mod tray;
mod windows;

use sth_voice::devices as keyboard;
use sth_voice::{listening, recorder, transcription, wav};

use broadcast::{Phase, Publish, Publishers, Socket, Status};
use daemon::{Daemon, Input, Ports};
use listening::Tuning;
use pin::{PinChange, Target};
use settings::Settings;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};
use transcription::{Transcriber, Whisper};

const RECONNECT: Duration = Duration::from_secs(1);
const ANSWER: Duration = Duration::from_millis(500);

const USAGE: &str = "sth-dictate: dictation against your own Whisper server. Tap the hotkey to start and again to stop,
or hold it and let go; the text is pasted at your cursor as one paragraph. Double tap to stream: every pause
sends what you said, with Enter. Double tap again to stop.

usage:
  sth-dictate serve     listen for the hotkey, record, transcribe and paste (run by the systemd user service)
  sth-dictate watch     print the state as one JSON line per change, reconnecting when the service restarts
  sth-dictate status    print the current state once
  sth-dictate test      send a second of silence to the endpoint and say how long it took
  sth-dictate pin       pin the window in front: dictation goes into it, then Enter, while you stay elsewhere
  sth-dictate pin herdr pin Herdr: the focused pane of the Herdr you used last
  sth-dictate pin off   stop pinning; dictation pastes at the cursor again
  sth-dictate pin on    pin again, to the last pinned window or Herdr
  sth-dictate --version

settings: ~/.config/sth-dictate/settings.json (endpoint, model, hotkey, hotkey_label, microphone, sounds, delivery,
paste_keys, pause_seconds, voice_level)";

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn whisper(settings: &Settings) -> Whisper {
    Whisper { endpoint: settings.endpoint.clone(), model: settings.model.clone() }
}

fn serve(settings: Settings) -> Result<(), String> {
    let (sender, inbox) = channel();
    let socket = Socket::bind(&broadcast::socket_path(), command_inbox(sender.clone()))?;
    keyboard::listen(vec![settings.hotkey], keyboard_inbox(sender.clone()));
    let tray: Vec<Box<dyn Publish>> =
        tray::show(sender.clone()).map(|icon| vec![Box::new(icon) as Box<dyn Publish>]).unwrap_or_else(|message| {
            eprintln!("{message}");
            vec![]
        });
    let pin_file = pin::file(&home());
    let pinned = pin::load(&pin_file);
    let microphone = || recorder::PipeWire { target: settings.microphone.clone() };
    let ports = Ports {
        hotkey: settings.hotkey,
        hotkey_label: settings.hotkey_label.clone(),
        microphone: Box::new(microphone()),
        ear: Box::new(microphone()),
        tuning: Tuning::new(settings.pause_seconds, settings.voice_level),
        transcriber: Arc::new(whisper(&settings)),
        desktop: Box::new(desktop::Wayland {
            home: home(),
            sounds: settings.sounds_dir(&home()),
            delivery: settings.delivery,
            paste_keys: settings.paste_keys.clone(),
            keyboard: desktop::Keyboard::create("sth-dictate keyboard")?,
        }),
        publish: Box::new(Publishers(std::iter::once(Box::new(socket) as Box<dyn Publish>).chain(tray).collect())),
        remember_pin: Box::new(move |pin| pin::save(&pin_file, pin)),
        inbox: sender,
    };
    inbox.iter().fold(Daemon::starting(pinned), |daemon, input| daemon.handle(input, &ports));
    Ok(())
}

fn keyboard_inbox(inbox: Sender<Input>) -> Sender<keyboard::Heard> {
    let (heard, from_keyboard) = channel();
    std::thread::spawn(move || {
        from_keyboard.iter().take_while(|event| inbox.send(Input::Heard(*event)).is_ok()).for_each(drop)
    });
    heard
}

fn command_inbox(inbox: Sender<Input>) -> Sender<String> {
    let (command, from_socket) = channel::<String>();
    std::thread::spawn(move || {
        from_socket
            .iter()
            .filter_map(|line| pin::command(&line))
            .take_while(|change| inbox.send(Input::Pin(change.clone())).is_ok())
            .for_each(drop)
    });
    command
}

fn stopped() -> String {
    broadcast::line(&Status {
        state: Phase::Unavailable,
        message: "sth-dictate is not running".into(),
        pinned: false,
        pinned_to: String::new(),
        target: None,
    })
}

fn window_in_front() -> Result<Target, String> {
    let window = windows::active().ok_or("No window is in front to pin")?;
    Ok(Target::Window { address: window.address, class: window.class, title: window.title })
}

fn pin_change(word: &str) -> Result<PinChange, String> {
    match word {
        "" | "here" => window_in_front().map(PinChange::To),
        "herdr" => Ok(PinChange::To(Target::Herdr)),
        "on" => Ok(PinChange::On),
        "off" => Ok(PinChange::Off),
        "toggle" => Ok(PinChange::Toggle),
        other => Err(format!("pin takes nothing (the window in front), herdr, on, off or toggle, not \"{other}\"")),
    }
}

fn pin(word: &str) -> Result<(), String> {
    let change = pin_change(word)?;
    let mut stream =
        UnixStream::connect(broadcast::socket_path()).map_err(|_| "sth-dictate is not running".to_string())?;
    stream
        .write_all(change.line().as_bytes())
        .and_then(|_| stream.set_read_timeout(Some(ANSWER)))
        .map_err(|error| format!("Could not reach sth-dictate: {error}"))?;
    let status = BufReader::new(stream)
        .lines()
        .map_while(Result::ok)
        .last()
        .and_then(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        .ok_or_else(|| "sth-dictate did not answer".to_string())?;
    let pinned_to = status.get("pinned_to").and_then(serde_json::Value::as_str).unwrap_or_default();
    if pinned_to.is_empty() {
        return print_now("Not pinned: dictation pastes at the cursor\n");
    }
    print_now(&format!("Pinned to {pinned_to}: dictation goes there, then Enter\n"))
}

fn watch() -> Result<(), String> {
    loop {
        if let Ok(stream) = UnixStream::connect(broadcast::socket_path()) {
            BufReader::new(stream)
                .lines()
                .map_while(Result::ok)
                .try_for_each(|line| print_now(&format!("{line}\n")))?;
        }
        print_now(&stopped())?;
        std::thread::sleep(RECONNECT);
    }
}

fn print_now(text: &str) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes()).and_then(|_| out.flush()).map_err(|error| error.to_string())
}

fn status() -> Result<(), String> {
    let first = UnixStream::connect(broadcast::socket_path())
        .ok()
        .and_then(|stream| BufReader::new(stream).lines().next())
        .and_then(Result::ok)
        .map(|line| format!("{line}\n"));
    print_now(&first.unwrap_or_else(stopped))
}

fn test(settings: &Settings) -> Result<(), String> {
    let started = Instant::now();
    whisper(settings).transcribe(&wav::wav(&wav::silence(1.0)))?;
    print_now(&format!("Endpoint OK, answered in {} ms\n", started.elapsed().as_millis()))
}

fn main() -> ExitCode {
    let command = std::env::args().nth(1).unwrap_or_default();
    let settings = || Settings::load(&home());
    let outcome = match command.as_str() {
        "serve" => settings().and_then(serve),
        "watch" => watch(),
        "status" => status(),
        "test" => settings().and_then(|settings| test(&settings)),
        "pin" => pin(&std::env::args().nth(2).unwrap_or_default()),
        "--version" => print_now(&format!("sth-dictate {}\n", env!("CARGO_PKG_VERSION"))),
        _ => Err(USAGE.into()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
