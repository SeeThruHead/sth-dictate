use crate::broadcast::{Phase, Publish, Status};
use crate::daemon::Input;
use crate::pin::{PinChange, Target};
use crate::windows::{self, Window};
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{MenuItem, ToolTip};
use std::sync::mpsc::Sender;
use std::time::Duration;

const REFRESH: Duration = Duration::from_secs(3);
const LONGEST_TITLE: usize = 48;

pub struct Tray {
    status: Status,
    windows: Vec<Window>,
    inbox: Sender<Input>,
}

pub fn window_label(window: &Window) -> String {
    let title: String = window.title.chars().take(LONGEST_TITLE).collect();
    let cut = if window.title.chars().count() > LONGEST_TITLE { "…" } else { "" };
    match (window.class.is_empty(), title.is_empty()) {
        (true, _) => format!("{title}{cut}"),
        (false, true) => window.class.clone(),
        (false, false) => format!("{}: {title}{cut}", window.class),
    }
}

pub fn pinnable(windows: Vec<Window>) -> Vec<Window> {
    windows.into_iter().filter(|window| !window.address.is_empty() && !window.class.is_empty()).collect()
}

fn target_of(window: &Window) -> Target {
    Target::Window { address: window.address.clone(), class: window.class.clone(), title: window.title.clone() }
}

fn is_pinned(target: &Option<Target>, window: &Window) -> bool {
    matches!(target, Some(Target::Window { address, .. }) if *address == window.address)
}

fn icon(state: &Phase) -> &'static str {
    match state {
        Phase::Recording => "media-record",
        Phase::Streaming => "audio-input-microphone-high",
        Phase::Unavailable => "microphone-sensitivity-muted",
        _ => "audio-input-microphone",
    }
}

impl Tray {
    fn send(&self, change: PinChange) {
        let _ = self.inbox.send(Input::Pin(change));
    }

    fn header(&self) -> Vec<MenuItem<Self>> {
        let pinned = match &self.status.target {
            Some(target) => format!("Pinned to {}: dictation goes there, then Enter", target.label()),
            None => "Not pinned: dictation pastes at the cursor".into(),
        };
        [self.status.message.clone(), pinned]
            .into_iter()
            .map(|label| StandardItem { label, enabled: false, ..Default::default() }.into())
            .collect()
    }

    fn window_items(&self) -> Vec<MenuItem<Self>> {
        self.windows
            .iter()
            .map(|window| {
                let target = target_of(window);
                CheckmarkItem {
                    label: format!("Pin to {}", window_label(window)),
                    checked: is_pinned(&self.status.target, window),
                    activate: Box::new(move |this: &mut Self| this.send(PinChange::To(target.clone()))),
                    ..Default::default()
                }
                .into()
            })
            .collect()
    }

    fn choices(&self) -> Vec<MenuItem<Self>> {
        let herdr = CheckmarkItem {
            label: "Pin to Herdr (its focused pane)".into(),
            checked: self.status.target == Some(Target::Herdr),
            activate: Box::new(|this: &mut Self| this.send(PinChange::To(Target::Herdr))),
            ..Default::default()
        };
        let unpin = StandardItem {
            label: "Unpin".into(),
            enabled: self.status.pinned,
            activate: Box::new(|this: &mut Self| this.send(PinChange::Off)),
            ..Default::default()
        };
        vec![herdr.into(), unpin.into()]
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "sth-dictate".into()
    }

    fn title(&self) -> String {
        "STH Dictate".into()
    }

    fn icon_name(&self) -> String {
        icon(&self.status.state).into()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: "STH Dictate".into(), description: self.status.message.clone(), ..Default::default() }
    }

    fn menu_about_to_show(&mut self) {
        self.windows = pinnable(windows::all());
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        [self.header(), vec![MenuItem::Separator], self.window_items(), vec![MenuItem::Separator], self.choices()]
            .into_iter()
            .flatten()
            .collect()
    }
}

pub struct TrayIcon(Handle<Tray>);

impl Publish for TrayIcon {
    fn publish(&self, status: &Status) {
        let status = status.clone();
        self.0.update(move |tray: &mut Tray| tray.status = status);
    }
}

pub fn show(inbox: Sender<Input>) -> Result<TrayIcon, String> {
    let status = Status {
        state: Phase::Idle,
        message: "Starting".into(),
        pinned: false,
        pinned_to: String::new(),
        target: None,
    };
    let handle = Tray { status, windows: pinnable(windows::all()), inbox }
        .assume_sni_available(true)
        .spawn()
        .map_err(|error| format!("Could not show the tray icon: {error}"))?;
    keep_windows_fresh(handle.clone());
    Ok(TrayIcon(handle))
}

fn keep_windows_fresh(handle: Handle<Tray>) {
    std::thread::spawn(move || {
        std::iter::repeat(()).try_fold(Vec::new(), |shown: Vec<Window>, ()| {
            std::thread::sleep(REFRESH);
            let current = pinnable(windows::all());
            if current != shown {
                handle.update({
                    let current = current.clone();
                    move |tray: &mut Tray| tray.windows = current
                })?;
            }
            Some(current)
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(address: &str, class: &str, title: &str) -> Window {
        Window { address: address.into(), class: class.into(), title: title.into(), ..Window::default() }
    }

    #[test]
    fn a_window_reads_as_its_app_and_title() {
        assert_eq!(window_label(&window("0x1", "Slack", "general")), "Slack: general");
        assert_eq!(window_label(&window("0x1", "kitty", "")), "kitty");
        let long = "x".repeat(60);
        assert_eq!(window_label(&window("0x1", "kitty", &long)), format!("kitty: {}…", "x".repeat(48)));
    }

    #[test]
    fn only_real_windows_can_be_pinned() {
        let windows = vec![window("0x1", "Slack", "general"), window("", "ghost", ""), window("0x2", "", "untitled")];
        assert_eq!(pinnable(windows), [window("0x1", "Slack", "general")]);
    }

    #[test]
    fn the_pinned_window_is_checked() {
        let slack = window("0x1", "Slack", "general");
        assert!(is_pinned(&Some(target_of(&slack)), &slack));
        assert!(!is_pinned(&Some(Target::Herdr), &slack));
        assert!(!is_pinned(&None, &slack));
    }
}
