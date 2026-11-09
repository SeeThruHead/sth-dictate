use crate::listening::{FRAME, Heard, Listener, Tuning};
use crate::wav;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;

const SIGINT: i32 = 2;

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

pub trait Capture: Send {
    fn finish(self: Box<Self>) -> Vec<i16>;
}

pub trait Microphone: Send {
    fn start(&self) -> Result<Box<dyn Capture>, String>;
}

pub struct PipeWire {
    pub target: Option<String>,
}

struct Recording {
    child: Child,
    collected: JoinHandle<Vec<u8>>,
}

impl Microphone for PipeWire {
    fn start(&self) -> Result<Box<dyn Capture>, String> {
        let mut child = self.record()?;
        let mut stdout = child.stdout.take().ok_or("pw-record gave no output")?;
        let collected = std::thread::spawn(move || {
            let mut bytes = vec![];
            stdout.read_to_end(&mut bytes).map(|_| bytes).unwrap_or_default()
        });
        Ok(Box::new(Recording { child, collected }))
    }
}

impl Capture for Recording {
    fn finish(self: Box<Self>) -> Vec<i16> {
        let Recording { mut child, collected } = *self;
        unsafe { kill(child.id() as i32, SIGINT) };
        let _ = child.wait();
        wav::samples_from_bytes(&collected.join().unwrap_or_default())
    }
}

pub trait Stream: Send {}

pub trait Ear: Send {
    fn listen(&self, tuning: Tuning, heard: Box<dyn Fn(Heard) -> bool + Send>) -> Result<Box<dyn Stream>, String>;
}

struct Listening {
    child: Child,
}

impl Stream for Listening {}

impl Drop for Listening {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl PipeWire {
    pub fn record(&self) -> Result<Child, String> {
        let rate = wav::SAMPLE_RATE.to_string();
        let target = self.target.iter().flat_map(|target| ["--target", target.as_str()]);
        let args =
            ["--raw", "--rate", &rate, "--channels", "1", "--format", "s16"].into_iter().chain(target).chain(["-"]);
        Command::new("pw-record")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start pw-record: {error}"))
    }
}

pub fn frames(mut stdout: impl Read) -> impl Iterator<Item = Vec<i16>> {
    std::iter::from_fn(move || {
        let mut bytes = [0u8; FRAME * 2];
        stdout.read_exact(&mut bytes).ok().map(|_| wav::samples_from_bytes(&bytes))
    })
}

impl Ear for PipeWire {
    fn listen(&self, tuning: Tuning, heard: Box<dyn Fn(Heard) -> bool + Send>) -> Result<Box<dyn Stream>, String> {
        let mut child = self.record()?;
        let stdout = child.stdout.take().ok_or("pw-record gave no output")?;
        std::thread::spawn(move || {
            let _ = frames(stdout).try_fold(Listener::default(), |listener, frame| {
                let (next, event) = listener.hear(frame, tuning);
                let delivered = event.is_none_or(&heard);
                delivered.then_some(next)
            });
        });
        Ok(Box::new(Listening { child }))
    }
}
