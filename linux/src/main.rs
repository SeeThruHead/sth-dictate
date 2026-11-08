mod broadcast;
mod daemon;
mod desktop;
mod herdr;
mod hotkey;
mod keyboard;
mod recorder;
mod session;
mod settings;
mod transcription;
mod wav;

use broadcast::{Phase, Socket, Status};
use daemon::{Daemon, Input, Ports};
use herdr::PinChange;
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
or hold it and let go; the text is pasted at your cursor as one paragraph.

usage:
  sth-dictate serve     listen for the hotkey, record, transcribe and paste (run by the systemd user service)
  sth-dictate watch     print the state as one JSON line per change, reconnecting when the service restarts
  sth-dictate status    print the current state once
  sth-dictate test      send a second of silence to the endpoint and say how long it took
  sth-dictate pin [on|off|toggle]
                        send dictated text to the focused Herdr pane instead of the cursor (toggle by default)
  sth-dictate --version

settings: ~/.config/sth-dictate/settings.json (endpoint, model, hotkey, hotkey_label, microphone, sounds, delivery, paste_keys)";

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn whisper(settings: &Settings) -> Whisper {
    Whisper { endpoint: settings.endpoint.clone(), model: settings.model.clone() }
}

fn serve(settings: Settings) -> Result<(), String> {
    let (sender, inbox) = channel();
    let socket = Socket::bind(&broadcast::socket_path(), command_inbox(sender.clone()))?;
    keyboard::listen(settings.hotkey, keyboard_inbox(sender.clone()));
    let pin_file = herdr::pin_file(&home());
    let pinned = pin_file.exists();
    let ports = Ports {
        hotkey: settings.hotkey,
        hotkey_label: settings.hotkey_label.clone(),
        microphone: Box::new(recorder::PipeWire { target: settings.microphone.clone() }),
        transcriber: Arc::new(whisper(&settings)),
        desktop: Box::new(desktop::Wayland {
            sounds: settings.sounds_dir(&home()),
            delivery: settings.delivery,
            paste_keys: settings.paste_keys.clone(),
            keyboard: desktop::Keyboard::create()?,
        }),
        publish: Box::new(socket),
        remember_pin: Box::new(move |pinned| herdr::remember(&pin_file, pinned)),
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
            .filter_map(|line| herdr::command(&line))
            .take_while(|change| inbox.send(Input::Pin(*change)).is_ok())
            .for_each(drop)
    });
    command
}

fn stopped() -> String {
    broadcast::line(&Status { state: Phase::Unavailable, message: "sth-dictate is not running".into(), pinned: false })
}

fn pin(word: &str) -> Result<(), String> {
    let change = PinChange::parse(word).ok_or_else(|| format!("pin takes on, off or toggle, not \"{word}\""))?;
    let mut stream =
        UnixStream::connect(broadcast::socket_path()).map_err(|_| "sth-dictate is not running".to_string())?;
    stream
        .write_all(format!("pin {}\n", change.word()).as_bytes())
        .and_then(|_| stream.set_read_timeout(Some(ANSWER)))
        .map_err(|error| format!("Could not reach sth-dictate: {error}"))?;
    let pinned = BufReader::new(stream)
        .lines()
        .map_while(Result::ok)
        .last()
        .and_then(|line| serde_json::from_str::<serde_json::Value>(&line).ok())
        .and_then(|status| status.get("pinned").and_then(serde_json::Value::as_bool))
        .ok_or_else(|| "sth-dictate did not say whether it is pinned".to_string())?;
    let said = if pinned {
        "Pinned to Herdr: text goes to the focused Herdr pane\n"
    } else {
        "Not pinned: text is pasted at the cursor\n"
    };
    print_now(said)
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
