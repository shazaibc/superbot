# 0004: MiniMax adapter and scoped Bot tokens

Date: 2026-11-23. Status: accepted.

## Decision

- **MiniMax adapter (`server/src/adapters/minimax.rs`)**: runs `claude` CLI against MiniMax's Anthropic-compatible endpoint (`https://api.minimax.io/anthropic`) using `MINIMAX_API_KEY`, keeping the shared `stream-json` loop and ADR 0001's "no custom agent loop" rule.
- **Scoped per-Bot tokens**: `botd` issues a random bearer token per Bot and restricts `Bearer` requests to `botctl` routes matching that Bot's ID, instead of granting full admin API access.
- **Auth & secret scrubbing**: `AUTH_PASSWORD` is required by default (unless `ALLOW_NO_AUTH=1` on loopback), stripped from Bot child environments, and paired with `Origin`/`Referer` checks on state-changing browser requests.
