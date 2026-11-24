//! The only contract between the server and a harness (Claude Code, Codex, Pi).
//! Each adapter builds a command line and maps the harness's JSON lines to `AgentEvent`s.
//! Spawning, streaming, stopping, and exit handling are shared here.

mod claude;
mod codex;
mod minimax;
mod pi;

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;

pub enum AgentEvent {
    /// Harness session to resume next time.
    Session(String),
    /// Streaming piece of the current text block.
    TextDelta(String),
    /// A finished text block: one chat bubble.
    Text(String),
    /// The agent started using a tool.
    Tool { name: String, detail: String },
    Error(String),
}

pub struct RunInput {
    pub prompt: String,
    /// Bot identity, rules, and memory. Rebuilt every run.
    pub system: String,
    /// None = start a new harness session.
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pub model: Option<String>,
    /// Reasoning effort, in the harness's own words (e.g. "high").
    pub effort: Option<String>,
    pub env: HashMap<String, String>,
    /// A private folder for this Bot's harness files.
    pub state_dir: PathBuf,
}

pub trait Adapter: Send {
    fn bin(&self) -> &'static str;
    /// Command-line arguments. May emit events up front (e.g. a session id it chose).
    fn args(&mut self, input: &RunInput, out: &mut Vec<AgentEvent>) -> Vec<String>;
    /// Map one JSON line from the harness to zero or more events.
    fn on_event(&mut self, e: &Value, out: &mut Vec<AgentEvent>);
    /// Models this harness can run here, with their reasoning efforts. Blocking.
    fn models(&self) -> Vec<ModelOption>;
    /// Harness-specific environment overrides for the child process.
    fn env(&self, _input: &RunInput) -> HashMap<String, String> {
        HashMap::new()
    }
    /// Environment variables to scrub from the child process before applying `env()`.
    fn env_remove(&self) -> &'static [&'static str] {
        &[]
    }
}

#[derive(Clone, serde::Serialize)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    /// Efforts this model accepts. Empty = no effort setting.
    pub efforts: Vec<String>,
    /// Other names the harness accepts for this model (e.g. "opus").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

/// Model lists, cached for 10 minutes per harness (listing can take a second).
pub fn models(name: &str) -> Vec<ModelOption> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<HashMap<String, (Instant, Vec<ModelOption>)>>> = Mutex::new(None);
    if let Some((at, list)) = CACHE.lock().unwrap().get_or_insert_default().get(name) {
        if at.elapsed() < Duration::from_secs(600) && !list.is_empty() {
            return list.clone();
        }
    }
    let list = get(name).map(|a| a.models()).unwrap_or_default();
    CACHE.lock().unwrap().get_or_insert_default().insert(name.into(), (Instant::now(), list.clone()));
    list
}

/// Runs a command and returns its stdout, or "" if it fails.
pub fn output(bin: &str, args: &[&str]) -> String {
    std::process::Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// To add a harness: write one adapter file and list it here.
pub const NAMES: [&str; 4] = ["minimax", "claude", "codex", "pi"];

pub fn get(name: &str) -> Option<Box<dyn Adapter>> {
    match name {
        "minimax" => Some(Box::new(minimax::MiniMax::default())),
        "claude" => Some(Box::new(claude::Claude::default())),
        "codex" => Some(Box::new(codex::Codex)),
        "pi" => Some(Box::new(pi::Pi)),
        _ => None,
    }
}

/// Harnesses whose CLI is on PATH.
pub fn installed() -> Vec<&'static str> {
    NAMES
        .into_iter()
        .filter(|n| crate::screen::on_path(get(n).unwrap().bin()))
        .collect()
}

/// Why a run was cut short.
pub type AbortReason = Option<&'static str>;

/// Runs the harness, calling `emit` for every event, until it exits or `abort` fires.
pub async fn run(
    mut adapter: Box<dyn Adapter>,
    input: RunInput,
    mut abort: watch::Receiver<AbortReason>,
    mut emit: impl FnMut(AgentEvent),
) {
    let mut out = Vec::new();
    let args = adapter.args(&input, &mut out);
    let adapter_env = adapter.env(&input);
    let adapter_env_remove = adapter.env_remove();
    out.drain(..).for_each(&mut emit);

    let mut cmd = Command::new(adapter.bin());
    cmd.args(&args)
        .current_dir(&input.cwd)
        .env_remove("AUTH_PASSWORD");
    for k in adapter_env_remove {
        cmd.env_remove(k);
    }
    cmd.envs(&input.env)
        .envs(&adapter_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0) // own group, so stop kills the whole tree
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return emit(AgentEvent::Error(format!("Couldn't start {}: {e}", adapter.bin()))),
    };
    let pid = child.id();

    let mut stderr = child.stderr.take().unwrap();
    let stderr_task = tokio::spawn(async move {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s).await;
        s
    });

    // JSON lines. Split only on "\n": some harnesses put U+2028 inside JSON strings.
    let mut lines = BufReader::new(child.stdout.take().unwrap()).split(b'\n');
    let mut saw_error = false;
    let mut aborted = false;
    loop {
        tokio::select! {
            line = lines.next_segment() => match line {
                Ok(Some(bytes)) => {
                    let Ok(v) = serde_json::from_slice::<Value>(&bytes) else { continue };
                    adapter.on_event(&v, &mut out);
                    for e in out.drain(..) {
                        saw_error |= matches!(e, AgentEvent::Error(_));
                        emit(e);
                    }
                }
                _ => break,
            },
            _ = abort.wait_for(|r| r.is_some()), if !aborted => {
                aborted = true;
                if let Some(pid) = pid {
                    kill_group(pid, "TERM");
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        kill_group(pid, "KILL");
                    });
                }
            }
        }
    }
    let status = child.wait().await;
    let stderr = stderr_task.await.unwrap_or_default();
    let ok = status.as_ref().is_ok_and(|s| s.success());
    if !ok && !saw_error && !aborted {
        let tail: Vec<&str> = stderr.trim().lines().rev().take(3).collect();
        let msg = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        emit(AgentEvent::Error(if msg.is_empty() {
            format!("{} exited with {}", adapter.bin(), status.map(|s| s.to_string()).unwrap_or_default())
        } else {
            msg
        }));
    }
}

fn kill_group(pid: u32, signal: &str) {
    let _ = std::process::Command::new("kill")
        .args([format!("-{signal}"), "--".into(), format!("-{pid}")])
        .stderr(Stdio::null())
        .status();
}

/// One short line describing a tool call, for the "working" status.
pub fn describe(input: &Value) -> String {
    let v = ["command", "cmd", "file_path", "path", "url", "query", "pattern", "description"]
        .iter()
        .find_map(|k| input.get(*k));
    let s = match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    };
    short(&s)
}

pub fn short(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect()
}

pub fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn new_uuid() -> String {
    let mut b = crate::random_bytes(16);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}
