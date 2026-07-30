# Hamba Timer 🦀 🐶

![Demo](demo.png)

A terminal-based work timer with a retro computer pet companion — and an MCP server so your AI agent can see what you're up to during the workday and talk back through the pet.

## Features

*   **Computer Companion**: Animated ASCII art that reacts to your state (Working/Break/Idle), with a mood (focused / happy / tired / sleepy) derived from your work-break balance. The timer dashboard is stacked beneath it so the app works well in tall, narrow terminal panes.
*   **Work/Break Tracking**: Logs every session with start/end times. Sessions close honestly on quit — no inflated durations.
*   **Inline Journal**: Every timer block expands inside the work log to reveal timestamped journal bullets. Add, edit, and delete notes without leaving the table; starting a new timer block opens it and closes the previous one.
*   **Agent Integration (MCP)**: `hamba_timer serve` runs a stdio MCP server. Your agent can poll live status, today's summary, history, and weekly stats — and send you messages that pop up as speech bubbles from the pet.
*   **Persistence**: Data lives in your platform data dir (`~/Library/Application Support/pet-timer/` on macOS), written atomically. A legacy `./work_log.json` is migrated automatically on first run.

## Controls

| Key | Action |
| :--- | :--- |
| **Space** | Toggle between **Working** and **Break** |
| **s** | **Stop** (Idle mode - pauses tracking) |
| **n** / **j** | Expand the active timer block and start an inline journal bullet |
| **Enter** | Open a selected timer block, edit a selected bullet, or activate **+ add note**. While adding, Enter saves the bullet and opens the next blank one |
| **Alt+Enter** | Insert a newline while editing a journal bullet |
| **Ctrl+S** | Save the current bullet and leave editing |
| **m** | Dismiss agent **message** bubble |
| **r** | **Resume** the selected block — undo accidental toggles: delete junk blocks with `d`, `d`, then resume the real block (today only) |
| **d**, then **d** | Confirm deletion of the selected closed timer block or journal bullet |
| **Arrow Up/Down** | Select timer blocks, or navigate bullets inside an expanded block |
| **Arrow Left/Right** | Change Day (View past history) |
| **Esc** | Cancel a draft or deletion, close an expanded block, or clear selection |
| **q** | Quit |

## Installation

1.  Ensure you have Rust installed.
2.  Clone the repo.
3.  Run:
    ```bash
    cargo run --release        # try it
    cargo install --path .     # install (needed for the agent integration)
    ```

## Agent integration

The MCP server reads the same data files the TUI writes, so it answers correctly whether or not the TUI is open (a heartbeat in `status.json` tells it which). Tools:

| Tool | What the agent gets |
| :--- | :--- |
| `get_current_status` | Live state, mood, session elapsed, today totals, latest journal entry |
| `get_today_summary` | Totals, ratio, session count, full journal timeline |
| `get_history` | One day's sessions in detail, or per-day summaries over a range |
| `get_weekly_stats` | 7-day bars, streak, week total, best day |
| `send_message` | Shows a speech bubble from the pet in the TUI (read receipts via status) |

Example config for [Hermes](https://hermes-agent.nousresearch.com/docs) — in `~/.hermes/config.yaml`, or `~/.hermes/profiles/<name>/config.yaml` if you use profiles (profile configs are standalone; the root config is NOT inherited, so register the server in the config of the profile that should see it):

```yaml
mcp_servers:
  hamba_timer:
    command: /Users/you/.cargo/bin/hamba_timer
    args: [serve]
    tools:
      prompts: false
      resources: false
```

If your config defines `platform_toolsets` allowlists, also add `hamba_timer` to each platform that should expose the tools (e.g. under `platform_toolsets.cli` and `platform_toolsets.telegram`) — Hermes gates MCP servers per platform by server name.

Any MCP-capable client (Claude Code, etc.) can connect the same way: command `hamba_timer`, args `["serve"]`, stdio transport.
