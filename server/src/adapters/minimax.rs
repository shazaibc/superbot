//! MiniMax via Claude Code CLI pointed at MiniMax's Anthropic-compatible API
//! (`https://api.minimax.io/anthropic`). Needs `claude` on PATH and `MINIMAX_API_KEY`.

use super::{claude, Adapter, AgentEvent, ModelOption, RunInput};
use serde_json::Value;
use std::collections::HashMap;

const DEFAULT_BASE_URL: &str = "https://api.minimax.io/anthropic";
const DEFAULT_MODEL: &str = "MiniMax-M3.1-Flash";

#[derive(Default)]
pub struct MiniMax {
    id: String,
    saw_turn: bool,
}

fn env_non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

impl Adapter for MiniMax {
    fn bin(&self) -> &'static str {
        "claude"
    }

    fn env(&self, input: &RunInput) -> HashMap<String, String> {
        let base_url = env_non_empty("MINIMAX_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into());
        let model = input
            .model
            .clone()
            .or_else(|| env_non_empty("MINIMAX_MODEL"))
            .unwrap_or_else(|| DEFAULT_MODEL.into());
        let mut m = HashMap::from([
            ("ANTHROPIC_BASE_URL".into(), base_url),
            ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(), "1".into()),
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW".into(), "524288".into()),
            ("ANTHROPIC_MODEL".into(), model.clone()),
            ("ANTHROPIC_DEFAULT_SONNET_MODEL".into(), model.clone()),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL".into(), model.clone()),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL".into(), model),
        ]);
        if let Some(key) = env_non_empty("MINIMAX_API_KEY").or_else(|| env_non_empty("ANTHROPIC_API_KEY")) {
            m.insert("ANTHROPIC_API_KEY".into(), key);
        }
        m
    }

    fn env_remove(&self) -> &'static [&'static str] {
        &["ANTHROPIC_AUTH_TOKEN", "MINIMAX_API_KEY"]
    }

    fn args(&mut self, input: &RunInput, _out: &mut Vec<AgentEvent>) -> Vec<String> {
        let (id, args) = claude::build_args(
            input,
            Some(&env_non_empty("MINIMAX_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into())),
        );
        self.id = id;
        args
    }

    fn on_event(&mut self, e: &Value, out: &mut Vec<AgentEvent>) {
        claude::handle_event(&self.id, &mut self.saw_turn, e, out);
    }

    fn models(&self) -> Vec<ModelOption> {
        let efforts: Vec<String> = ["low", "medium", "high", "xhigh", "max"].map(String::from).into();
        [
            ("MiniMax-M3.1-Flash", "MiniMax M3.1 Flash"),
            ("MiniMax-M3", "MiniMax M3"),
        ]
        .map(|(id, label)| ModelOption {
            id: id.into(),
            label: label.into(),
            efforts: efforts.clone(),
            aliases: vec![],
        })
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn minimax_defaults_and_models() {
        let mut m = MiniMax::default();
        assert_eq!(m.bin(), "claude");
        let models = m.models();
        assert_eq!(models[0].id, "MiniMax-M3.1-Flash");
        assert_eq!(models[1].id, "MiniMax-M3");

        let input = RunInput {
            prompt: "hi".into(),
            system: "sys".into(),
            session_id: None,
            cwd: PathBuf::from("."),
            model: None,
            effort: None,
            env: HashMap::new(),
            state_dir: PathBuf::from("."),
        };
        let env = m.env(&input);
        assert_eq!(env.get("ANTHROPIC_BASE_URL").map(String::as_str), Some("https://api.minimax.io/anthropic"));
        assert_eq!(env.get("ANTHROPIC_MODEL").map(String::as_str), Some("MiniMax-M3.1-Flash"));
        let mut out = Vec::new();
        let args = m.args(&input, &mut out);
        assert!(args.windows(2).any(|w| w == ["--model", "MiniMax-M3.1-Flash"]));
    }
}
