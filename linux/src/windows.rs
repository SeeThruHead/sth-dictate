use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::Command;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Window {
    pub address: String,
    pub class: String,
    pub title: String,
    pub pid: i32,
    pub xwayland: bool,
    #[serde(rename = "focusHistoryID")]
    pub focus_history: i64,
}

fn hyprctl(args: &[&str]) -> Option<Vec<u8>> {
    Command::new("hyprctl").args(args).output().ok().map(|output| output.stdout)
}

pub fn all() -> Vec<Window> {
    hyprctl(&["clients", "-j"]).and_then(|json| serde_json::from_slice(&json).ok()).unwrap_or_default()
}

pub fn active() -> Option<Window> {
    hyprctl(&["activewindow", "-j"])
        .and_then(|json| serde_json::from_slice::<Window>(&json).ok())
        .filter(|window| !window.address.is_empty())
}

pub fn shortcut(address: &str, mods: &str, code: u16) -> String {
    format!(
        "hl.dsp.send_shortcut({{ mods = \"{mods}\", key = \"code:{}\", window = \"address:{address}\" }})",
        code + 8
    )
}

pub fn send_shortcut(address: &str, mods: &str, code: u16) -> Result<(), String> {
    let answer = hyprctl(&["dispatch", &shortcut(address, mods, code)]).ok_or("Could not run hyprctl")?;
    let said = String::from_utf8_lossy(&answer).trim().to_string();
    if said.is_empty() || said == "ok" {
        return Ok(());
    }
    Err(format!("Hyprland could not send keys to the pinned window: {said}"))
}

pub fn parents() -> BTreeMap<i32, (i32, String)> {
    std::fs::read_dir("/proc")
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().to_string_lossy().parse::<i32>().ok())
                .filter_map(|pid| std::fs::read_to_string(format!("/proc/{pid}/stat")).ok().map(|stat| (pid, stat)))
                .filter_map(|(pid, stat)| parse_stat(&stat).map(|parent| (pid, parent)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_stat(stat: &str) -> Option<(i32, String)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let name = stat.get(open + 1..close)?.to_string();
    let parent = stat.get(close + 2..)?.split_whitespace().nth(1)?.parse().ok()?;
    Some((parent, name))
}

pub fn runs(processes: &BTreeMap<i32, (i32, String)>, root: i32, name: &str) -> bool {
    processes.iter().any(|(pid, (_, process))| process == name && descends_from(processes, *pid, root))
}

fn descends_from(processes: &BTreeMap<i32, (i32, String)>, pid: i32, root: i32) -> bool {
    std::iter::successors(Some(pid), |current| processes.get(current).map(|(parent, _)| *parent))
        .take(64)
        .take_while(|current| *current > 1)
        .any(|current| current == root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_names_the_key_code_and_the_window() {
        assert_eq!(
            shortcut("0x55ac", "SHIFT", 110),
            "hl.dsp.send_shortcut({ mods = \"SHIFT\", key = \"code:118\", window = \"address:0x55ac\" })"
        );
    }

    #[test]
    fn a_process_line_gives_its_parent_and_name() {
        assert_eq!(parse_stat("4242 (herdr) S 4100 4242 ..."), Some((4100, "herdr".into())));
        assert_eq!(parse_stat("7 (tmux: server) S 1 7"), Some((1, "tmux: server".into())));
    }

    #[test]
    fn a_window_runs_herdr_when_herdr_descends_from_its_process() {
        let processes: BTreeMap<i32, (i32, String)> = [
            (100, (1, "kitty".to_string())),
            (200, (100, "zsh".to_string())),
            (300, (200, "herdr".to_string())),
            (400, (1, "kitty".to_string())),
        ]
        .into_iter()
        .collect();
        assert!(runs(&processes, 100, "herdr"));
        assert!(!runs(&processes, 400, "herdr"));
    }

    #[test]
    fn hyprland_clients_parse_with_their_focus_order() {
        let json =
            r#"[{"address":"0x1","class":"kitty","title":"t","pid":5,"xwayland":false,"focusHistoryID":2,"extra":1}]"#;
        let windows: Vec<Window> = serde_json::from_str(json).unwrap();
        assert_eq!(windows[0].focus_history, 2);
        assert_eq!(windows[0].class, "kitty");
    }
}
