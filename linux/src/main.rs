mod broadcast;
mod daemon;
mod desktop;
mod hotkey;
mod keyboard;
mod recorder;
mod session;
mod settings;
mod transcription;
mod wav;

use broadcast::{Phase, Socket, Status};
use daemon::{Daemon, Input, Ports};
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

const USAGE: &str = "sth-dictate: dictation against your own Whisper server. Tap the hotkey to start and again to stop,
or hold it and let go; the text is pasted at your cursor as one paragraph.

usage:
  sth-dictate serve     listen for the hotkey, record, transcribe and paste (run by the systemd user service)
  sth-dictate watch     print the state as one JSON line per change, reconnecting when the service restarts
  sth-dictate status    print the current state once
  sth-dictate test      send a second of silence to the endpoint and say how long it took
  sth-dictate --version

settings: ~/.config/sth-dictate/settings.json (endpoint, model, hotkey, hotkey_label, microphone, sounds)";

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn whisper(settings: &Settings) -> Whisper {
    Whisper { endpoint: settings.endpoint.clone(), model: settings.model.clone() }
}

fn serve(settings: Settings) -> Result<(), String> {
    let socket = Socket::bind(&broadcast::socket_path())?;
    let (sender, inbox) = channel();
    keyboard::listen(settings.hotkey, keyboard_inbox(sender.clone()));
    let ports = Ports {
        hotkey: settings.hotkey,
        hotkey_label: settings.hotkey_label.clone(),
        microphone: Box::new(recorder::PipeWire { target: settings.microphone.clone() }),
        transcriber: Arc::new(whisper(&settings)),
        desktop: Box::new(desktop::Wayland { sounds: settings.sounds_dir(&home()) }),
        publish: Box::new(socket),
        inbox: sender,
    };
    inbox.iter().fold(Daemon::default(), |daemon, input| daemon.handle(input, &ports));
    Ok(())
}

fn keyboard_inbox(inbox: Sender<Input>) -> Sender<keyboard::Heard> {
    let (heard, from_keyboard) = channel();
    std::thread::spawn(move || {
        from_keyboard.iter().take_while(|event| inbox.send(Input::Heard(*event)).is_ok()).for_each(drop)
    });
    heard
}

fn stopped() -> String {
    broadcast::line(&Status { state: Phase::Unavailable, message: "sth-dictate is not running".into() })
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
