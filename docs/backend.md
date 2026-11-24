# Backend

One Rust binary, `botd`. It serves the web app, stores everything in SQLite, and runs each Bot on a coding-agent CLI.

## How it works

- **Bots** live in SQLite. Each Bot has a harness (`claude`, `codex`, or `pi`), an optional model, and a harness session id.
- **A message** is queued. Each Bot runs one task at a time, in order. Different Bots run in parallel.
- **A run** spawns the harness CLI in the shared workspace, streams its JSON events, and saves each text block as a chat bubble.
- **Live updates** go to the browser over one Server-Sent Events stream: new messages, streaming text, and what the Bot is doing ("Running: ls").
- **Memory** is plain files, re-sent in the system prompt every run. The Bot edits them itself:
  - `DATA_DIR/bots/<id>/MEMORY.md`: one Bot's job, rules, and work log.
  - `DATA_DIR/USER.md`: facts about the user, shared by all Bots.
- **The computer** is `WORKSPACE_DIR`, shared by all Bots, like Grok Bot's shared VM.
- **Routines** are cron schedules. Bots create them with the `botctl` CLI, which works from any harness's shell.
- **Bot to Bot**: a Bot runs `botctl ask "<Bot>" "<message>"`. The other Bot answers in its own chat and the reply prints back. If it takes over 9 minutes, the reply arrives later as a new message.
- **Model and effort** are saved per Bot. The web app shows a faint picker above the input. Unset means the harness default.
- **Choice cards**: a reply ending in a ` ```question ` JSON block becomes a multiple-choice card.
- **Restarts**: queued messages run after a restart. A task that was running is reported as interrupted, not re-run, so it can't repeat an action like sending an email.

## Harness adapters

Each adapter is one file in `server/src/adapters/`. It builds the command line and maps the CLI's JSON lines to five events: session, text delta, text, tool, error.

- **MiniMax**: uses `claude` CLI pointed at `https://api.minimax.io/anthropic` (`MINIMAX_API_KEY`, models `MiniMax-M3.1-Flash` and `MiniMax-M3`).
- **Claude Code**: `claude -p --output-format stream-json --resume <id>`. System prompt via `--append-system-prompt`, fresh every run.
- **Codex**: `codex exec --json`, then `codex exec resume <id>`. Instructions via `-c developer_instructions=...`. Codex only reads them when a session starts, so later memory edits reach it through the memory file.
- **Pi**: `pi --mode json --session-dir <bot dir> --session-id <id>`. System prompt via `--append-system-prompt`.

All four run with permission prompts off (`--dangerously-skip-permissions` and equivalents). The server is the computer, so treat it that way. Approvals are not built yet.

Add a harness: write one adapter file and add it to `get()` and `NAMES` in `adapters/mod.rs`.

## Run it

```sh
cargo build --release          # makes target/release/botd and target/release/botctl
cp .env.example .env           # fill in AUTH_PASSWORD and API key
set -a; . ./.env; set +a
./target/release/botd
```

Settings are in [.env.example](../.env.example). Keep `botctl` next to `botd`: the server puts that folder on the Bots' PATH.

## Deploying on a Linux VPS

- Keep your server IP, domain, and secrets out of this repo. They go in `.env` on the server.
- On Ubuntu 24.04+, `sudo deploy/setup-vps.sh` creates a dedicated `superbot` user, installs Node.js + `@anthropic-ai/claude-code` (for `minimax`/`claude`), Rust, Caddy, Bot screens, and `systemd` service templates ([Caddyfile.example](../deploy/Caddyfile.example), [botd.service.example](../deploy/botd.service.example)).
- Or install the harness CLIs manually **as the user that runs `botd`**: `minimax` (`claude` CLI + `MINIMAX_API_KEY`), `claude` (`claude login` or `ANTHROPIC_API_KEY`), `codex` (`codex login` or `OPENAI_API_KEY`), `pi` (provider API key, e.g. `OPENROUTER_API_KEY`). `AUTH_PASSWORD` is scrubbed from Bot child processes, and each Bot gets a scoped token limited to `botctl` routes.
- Run `botd` as a normal user, not root. Claude Code refuses to skip permissions as root.
- Run it under systemd with `Restart=always`. It shuts down cleanly on SIGTERM.
- Put HTTPS in front (e.g. Caddy) and always set `AUTH_PASSWORD` (required unless `ALLOW_NO_AUTH=1` on loopback). The browser logs in with any username and that password.
- Proxies must not buffer `/api/events` (it's an SSE stream).
- Set `TZ` to the user's time zone. Routines use it.
- Back up `DATA_DIR`. `WORKSPACE_DIR` is the Bots' scratch and files.
- `GET /healthz` returns `{"ok":true}` without auth.

## API

All JSON. Browser auth: HTTP Basic with `AUTH_PASSWORD`.

- `GET /api/state`: user name, installed harnesses, all Bots
- `GET /api/events`: SSE stream (`message`, `draft`, `status`, `bot`, `botDeleted`, `resync`)
- `POST /api/bots` `{name, harness?, model?, color?, shape?}`
- `PATCH /api/bots/:id` `{name?, hidden?, harness?, model?, effort?}`, `DELETE /api/bots/:id`
- `GET /api/harnesses/:name/models`: models and reasoning efforts the harness offers (Claude Code: aliases; Codex: `codex debug models`; Pi: `pi --list-models`)
- `GET|POST /api/bots/:id/messages` `{text, answer?: {messageId, option}}`
- `POST /api/bots/:id/read`, `POST /api/bots/:id/stop`
- `GET|POST /api/bots/:id/routines` `{name, cron, instruction}`
- `PATCH|DELETE /api/routines/:id` `{enabled?, name?, cron?, instruction?}`
- `GET /api/bots/:id/screen`: starts the Bot's screen and returns its live view URL
- `POST /api/handoff` `{from, text}`: `botctl handoff`; waits for the user's answer

## Not built yet

Approvals before risky actions, Vault for secrets, per-Bot resource limits, Slack.

Computer use (each Bot's screen, browser, and live view) is in [computer.md](computer.md).
