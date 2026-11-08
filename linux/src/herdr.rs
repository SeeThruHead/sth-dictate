use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn program(home: &Path, name: &str) -> String {
    let local = home.join(".local/bin").join(name);
    if local.exists() {
        return local.to_string_lossy().into_owned();
    }
    name.to_string()
}

fn run(home: &Path, name: &str, args: &[&str]) -> Result<Output, String> {
    Command::new(program(home, name))
        .args(args)
        .env_remove("HERDR_PANE_ID")
        .output()
        .map_err(|error| format!("Could not run {name}: {error}"))
}

fn succeeded(output: Output, what: &str) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    Err(format!("{what}: {}", String::from_utf8_lossy(&output.stderr).trim()))
}

fn json(output: &Output) -> Option<Value> {
    serde_json::from_slice(&output.stdout).ok()
}

pub fn pane_id(answer: &Value) -> Option<String> {
    answer.pointer("/result/pane/pane_id")?.as_str().map(str::to_string)
}

pub fn agent_in_pane(agents: &Value, pane: &str) -> Option<String> {
    agents
        .pointer("/result/agents")?
        .as_array()?
        .iter()
        .find(|agent| agent.get("pane_id").and_then(Value::as_str) == Some(pane))?
        .get("name")?
        .as_str()
        .map(str::to_string)
}

pub fn send_to_server(home: &Path, text: &str) -> Result<(), String> {
    let current = run(home, "herdr", &["pane", "current"])?;
    let pane = json(&current).as_ref().and_then(pane_id).ok_or("Herdr has no focused pane")?;
    let agents = run(home, "herdr", &["agent", "list"])?;
    match json(&agents).as_ref().and_then(|agents| agent_in_pane(agents, &pane)) {
        Some(name) => succeeded(run(home, "herdr", &["agent", "prompt", &name, text])?, "Herdr could not prompt"),
        None => {
            succeeded(run(home, "herdr", &["pane", "send-text", &pane, text])?, "Herdr could not send the text")?;
            succeeded(run(home, "herdr", &["pane", "send-keys", &pane, "enter"])?, "Herdr could not press Enter")
        }
    }
}

pub fn terminal_of_pane(listing: &Value, pane: &str) -> Option<String> {
    if listing.get("pane").and_then(Value::as_str) == Some(pane) {
        return listing.get("terminal").and_then(Value::as_str).map(str::to_string);
    }
    let children: Vec<&Value> = match listing {
        Value::Array(items) => items.iter().collect(),
        Value::Object(fields) => fields.values().collect(),
        _ => vec![],
    };
    children.into_iter().find_map(|child| terminal_of_pane(child, pane))
}

pub fn app_terminal(status: &Value, listing: &Value) -> Option<String> {
    let focused = status.get("focused_pane").and_then(Value::as_str);
    focused
        .and_then(|pane| terminal_of_pane(listing, pane))
        .or_else(|| status.get("agent").and_then(Value::as_str).map(str::to_string))
}

pub fn send_to_app(home: &Path, text: &str) -> Result<(), String> {
    let status = run(home, "herdr-app", &["status"])?;
    let status = json(&status).ok_or("The Herdr app is not open")?;
    let listing = run(home, "herdr-app", &["ls"])?;
    let terminal =
        app_terminal(&status, &json(&listing).unwrap_or(Value::Null)).ok_or("The Herdr app has no focused terminal")?;
    succeeded(run(home, "hc", &["send", &terminal, text])?, "hc could not send the text")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn the_focused_pane_comes_from_herdr_pane_current() {
        let answer = value(r#"{"result":{"pane":{"focused":true,"pane_id":"wB:p1"},"type":"pane_current"}}"#);
        assert_eq!(pane_id(&answer), Some("wB:p1".into()));
        assert_eq!(pane_id(&value(r#"{"error":"no server"}"#)), None);
    }

    #[test]
    fn an_agent_in_the_focused_pane_is_prompted_by_name() {
        let agents =
            value(r#"{"result":{"agents":[{"name":"homelab","pane_id":"w1:p1"},{"name":"chief","pane_id":"wB:p1"}]}}"#);
        assert_eq!(agent_in_pane(&agents, "wB:p1"), Some("chief".into()));
        assert_eq!(agent_in_pane(&agents, "w9:p9"), None);
    }

    #[test]
    fn the_app_sends_to_the_terminal_of_its_focused_pane_or_its_agent() {
        let listing =
            value(r#"{"spaces":[{"tabs":[{"panes":[{"pane":"p1","terminal":"t2"},{"pane":"p9","terminal":"t7"}]}]}]}"#);
        assert_eq!(app_terminal(&value(r#"{"focused_pane":"p9","agent":"t5"}"#), &listing), Some("t7".into()));
        assert_eq!(app_terminal(&value(r#"{"focused_pane":null,"agent":"t5"}"#), &listing), Some("t5".into()));
        assert_eq!(app_terminal(&value(r#"{"focused_pane":null,"agent":null}"#), &listing), None);
    }
}
