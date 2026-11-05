use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    Start,
    Stop,
}

pub trait Desktop: Send {
    fn paste(&self, text: &str) -> Result<(), String>;
    fn play(&self, sound: Sound);
}

pub struct Wayland {
    pub sounds: PathBuf,
}

impl Desktop for Wayland {
    fn paste(&self, text: &str) -> Result<(), String> {
        copy(text)?;
        let typed = Command::new("wtype")
            .args(["-M", "shift", "-k", "Insert", "-m", "shift"])
            .status()
            .map_err(|error| format!("Could not run wtype: {error}"))?;
        typed.success().then_some(()).ok_or_else(|| "wtype could not press Shift+Insert".to_string())
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
