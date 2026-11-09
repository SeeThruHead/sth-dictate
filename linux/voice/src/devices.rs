use crate::keys::{self, EVENT_SIZE, KeyEvent};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RESCAN: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug)]
pub enum Heard {
    Key(KeyEvent),
    Devices(usize),
}

pub fn carries_key(device: &Path, code: u16) -> bool {
    let name = device.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let capabilities = std::fs::read_to_string(format!("/sys/class/input/{name}/device/capabilities/key"));
    capabilities.is_ok_and(|bits| keys::has_key(&bits, code))
}

fn carries_any(device: &Path, codes: &[u16]) -> bool {
    codes.iter().any(|code| carries_key(device, *code))
}

fn event_devices() -> BTreeSet<PathBuf> {
    std::fs::read_dir("/dev/input")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("event")))
                .collect()
        })
        .unwrap_or_default()
}

pub fn listen(codes: Vec<u16>, heard: Sender<Heard>) {
    let reading: Arc<Mutex<BTreeSet<PathBuf>>> = Arc::default();
    std::thread::spawn(move || {
        loop {
            let fresh: Vec<PathBuf> = event_devices()
                .into_iter()
                .filter(|device| !reading.lock().is_ok_and(|reading| reading.contains(device)))
                .filter(|device| carries_any(device, &codes))
                .collect();
            fresh.into_iter().for_each(|device| read_device(device, reading.clone(), heard.clone()));
            let count = reading.lock().map(|reading| reading.len()).unwrap_or(0);
            if heard.send(Heard::Devices(count)).is_err() {
                return;
            }
            std::thread::sleep(RESCAN);
        }
    });
}

fn read_device(device: PathBuf, reading: Arc<Mutex<BTreeSet<PathBuf>>>, heard: Sender<Heard>) {
    let Ok(file) = File::open(&device) else { return };
    if let Ok(mut set) = reading.lock() {
        set.insert(device.clone());
    }
    std::thread::spawn(move || {
        records(file)
            .flat_map(|record| keys::decode_events(&record))
            .take_while(|event| heard.send(Heard::Key(*event)).is_ok())
            .for_each(drop);
        if let Ok(mut set) = reading.lock() {
            set.remove(&device);
        }
    });
}

fn records(mut file: File) -> impl Iterator<Item = [u8; EVENT_SIZE]> {
    std::iter::from_fn(move || {
        let mut record = [0u8; EVENT_SIZE];
        file.read_exact(&mut record).ok().map(|_| record)
    })
}
