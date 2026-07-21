use anyhow::Result;

fn main() -> Result<()> {
    let mode = std::env::args().nth(1);
    match mode.as_deref() {
        None => hamba_timer::tui::run(),
        Some("serve") => hamba_timer::mcp::serve(),
        Some("--help" | "-h" | "help") => {
            println!("hamba_timer — work timer with a desk pet + agent integration");
            println!();
            println!("usage:");
            println!("  hamba_timer         launch the TUI");
            println!("  hamba_timer serve   run the MCP server (stdio) for agents");
            println!();
            println!("data dir: ~/Library/Application Support/pet-timer (override: PET_TIMER_DATA_DIR)");
            Ok(())
        }
        Some(other) => {
            eprintln!("unknown mode '{other}' — try --help");
            std::process::exit(2);
        }
    }
}
