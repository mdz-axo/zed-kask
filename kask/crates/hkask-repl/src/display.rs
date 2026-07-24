use super::commands::{find_command, fuzzy_match_command};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Derive a short display abbreviation from a model name.
///
/// Strips the provider prefix (e.g., `DI/`, `OR/`, `OM/`), capitalizes the
/// first letter, and removes common variant suffixes (`-flash`, `-instruct`,
/// `-chat`, `-turbo`) for brevity. Used as the REPL/TUI prompt prefix so the
/// user sees which LLM they are interacting with.
pub fn model_abbrev(model: &str) -> String {
    let name = model.rsplit("/").next().unwrap_or(model);
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
    .trim_end_matches("-flash")
    .trim_end_matches("-instruct")
    .trim_end_matches("-chat")
    .trim_end_matches("-turbo")
    .to_string()
}
/// Banner shown during onboarding (simpler than the main banner)
pub fn print_onboarding_banner() {
    let body = "\x1b[1;36m";
    let dim = "\x1b[2;37m";
    let r = "\x1b[0m";

    println!();
    println!("{body}  __          {dim}    ___________    __    {r}");
    println!("{body} /  \\         {dim}   /  \\             /  \\   {r}");
    println!("{body}|    |        {dim}  |    |    KASK    |    |  {r}");
    println!("{body} \\__/         {dim}  |    |            |    |  {r}");
    println!("{dim}              {body}  |    |            |    |  {r}");
    println!("{dim}              {body}   \\__/~~~~~~~~~~~~\\__/   {r}");
    println!("  {dim}shadow{r}       {body}    hKask v{VERSION}{r}");
    println!();
    println!("{body}     A Minimal Viable Container for UserPods{r}");
    println!();
}

pub(super) fn print_banner(
    agent: &str,
    template: Option<&str>,
    model: &str,
    is_first_run: bool,
    degraded_servers: &[(String, String)],
) {
    let ghost = "\x1b[2;36m";
    let body = "\x1b[1;36m";
    let bright = "\x1b[1;37m";
    let dim = "\x1b[2;37m";
    let gold = "\x1b[1;33m";
    let r = "\x1b[0m";

    let eye_frames: &[&str] = &["center", "right", "center", "left"];

    for (i, gaze) in eye_frames.iter().enumerate() {
        if i > 0 {
            print!("\x1b[10A");
        }

        let eye = match *gaze {
            "right" => format!("{bright}.::{gold}>{bright}:.{r}"),
            "left" => format!("{bright}.:{gold}<{bright}::.{r}"),
            _ => format!("{bright}.:{dim}:{bright}:.{r}"),
        };

        println!("{ghost}  __          {body}    ___________    __    {r}");
        println!("{ghost} /  \\         {body}   /  \\   {eye}   /  \\   {r}");
        println!("{ghost}|    |        {body}  |    |           |    |  {r}");
        println!("{ghost} \\__/         {body}  |    |    KASK   |    |  {r}");
        println!("{ghost}              {body}  |    |           |    |  {r}");
        println!("{ghost}              {body}   \\__/~~~~~~~~~~~\\__/   {r}");
        println!("  {ghost}shadow{r}       {body}    hKask v{VERSION}{r}");
        println!();
        println!("{body}     A Minimal Viable Container for UserPods{r}");
        println!();

        std::io::Write::flush(&mut std::io::stdout()).ok();

        if i < eye_frames.len() - 1 {
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
    }

    println!(
        "  \x1b[1mAgent:\x1b[0m \x1b[1m{}\x1b[0m  \x1b[1mModel:\x1b[0m \x1b[1m{}\x1b[0m  \x1b[1mTemplate:\x1b[0m \x1b[1m{}\x1b[0m",
        agent,
        model,
        template.unwrap_or("auto-select")
    );

    if is_first_run {
        print_first_steps();
        print_mcp_prompt();
    } else {
        println!(
            "  \x1b[1;36m/help\x1b[0m for commands  \x1b[2m<TAB>\x1b[0m autocomplete  \x1b[2m/quit\x1b[0m exit"
        );
        print_mcp_prompt();
    }

    // Warn about degraded servers — capabilities the agent expects but doesn't have.
    if !degraded_servers.is_empty() {
        println!();
        for (server_id, err) in degraded_servers {
            println!(
                "  \x1b[31m\u{26a0}  {} unavailable:\x1b[0m \x1b[2m{}\x1b[0m",
                server_id, err
            );
        }
        println!(
            "  \x1b[2m   The agent may not function correctly. Check that MCP binaries are on PATH.\x1b[0m"
        );
    }

    println!();
}

/// First Steps guide — shown after onboarding on the user's first session.
/// Progressive disclosure: only the most essential commands, with a pointer
/// to /help and /start for deeper exploration.
pub(super) fn print_first_steps() {
    println!("  \x1b[1;33m━━ First Steps ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!();
    println!("  \x1b[1mGetting started:\x1b[0m");
    println!("  • Just type to chat — your userpod is ready");
    println!("  • \x1b[36m/help\x1b[0m    — see all available commands");
    println!("  • \x1b[36m/model\x1b[0m   — switch models anytime");
    println!("  • \x1b[36m/mcp\x1b[0m     — manage MCP server connections");
    println!("  • \x1b[36m/status\x1b[0m  — check system health and energy");
    println!("  • \x1b[36m/repl\x1b[0m    — customize inference settings");
    println!();
    println!("  \x1b[2mTry: \"What can you help me with?\"\x1b[0m");
    println!("  \x1b[2mType \x1b[36m/start\x1b[0m\x1b[2m for a guided tour.\x1b[0m");
    println!("  \x1b[1;33m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
}

/// MCP consent prompt — shown on every session start.
///
/// P2 (Affirmative Consent): The 9 core servers (filesystem, memory, condenser,
/// research, skill, curator, kanban, docproc, media) auto-start to form the
/// agent's autonomous nervous system. Specialized servers (communication,
/// companies, training) require explicit opt-in via /mcp start.
pub(super) fn print_mcp_prompt() {
    println!();
    println!(
        "  \x1b[2;33mℹ  Core MCP servers loaded — full sensory, memory, and actuation available.\x1b[0m"
    );
    println!(
        "  \x1b[2m   Type \x1b[36m/mcp list\x1b[0m\x1b[2m to browse, \x1b[36m/mcp start all\x1b[0m\x1b[2m to load communication/companies/training.\x1b[0m"
    );
}

pub(super) fn print_help() {
    println!();
    println!("\x1b[1mℏKask Commands\x1b[0m");
    println!();

    let categories = [
        ("Session", &["help", "quit", "clear", "history"] as &[&str]),
        ("Agent", &["agent", "agents", "pods", "ask"]),
        ("Model", &["model"]),
        ("System", &["status", "tools", "templates", "sovereignty"]),
        (
            "Governance",
            &["escalations", "resolve", "dismiss", "metacognition"],
        ),
        ("Onboarding", &["start", "feedback"]),
    ];

    for (category, cmds) in &categories {
        println!("  \x1b[1;33m{}\x1b[0m", category);
        for &cmd_name in *cmds {
            if let Some(cmd) = find_command(cmd_name) {
                let alias_str = if cmd.aliases.is_empty() {
                    String::new()
                } else {
                    format!(", /{}", cmd.aliases.join(", /"))
                };
                let args_str = if cmd.args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", cmd.args)
                };
                println!(
                    "    \x1b[36m/{}{}\x1b[0m{}  — {}",
                    cmd.primary, args_str, alias_str, cmd.about
                );
            }
        }
        println!();
    }

    println!("  \x1b[2mTip: /help <command> for details on a specific command\x1b[0m");
    println!();
}

pub(super) fn print_command_help(cmd_name: &str) {
    if let Some(cmd) = find_command(cmd_name) {
        println!();
        println!("  \x1b[1;36m/{} {}\x1b[0m", cmd.primary, cmd.args);
        if !cmd.aliases.is_empty() {
            println!("  Aliases: /{}", cmd.aliases.join(", /"));
        }
        println!("  {}", cmd.about);

        match cmd.primary {
            "ask" => {
                println!();
                println!("  \x1b[2m/ask ScholarBot What do you think?\x1b[0m");
                println!();
                println!("  Force a specific agent to respond directly.");
            }
            "agent" => {
                println!();
                println!("  \x1b[2m/agent\x1b[0m          — Show current agent");
                println!("  \x1b[2m/agents\x1b[0m         — List all available agents");
            }
            "model" => {
                println!();
                println!("  \x1b[2m/model\x1b[0m                    — Show current model");
                println!("  \x1b[2m/model list\x1b[0m               — List all available models");
                println!(
                    "  \x1b[2m/model refresh\x1b[0m           — Re-fetch the model catalog from providers"
                );
                println!("  \x1b[2m/model qwen3:8b\x1b[0m       — Switch to a specific model");
                println!(
                    "  \x1b[2m/model qwen\x1b[0m            — Fuzzy search for models matching 'qwen'"
                );
                println!();
                println!(
                    "  Models are loaded from configured providers. Use a model name from /model"
                );
                println!("  to change the LLM used by the current agent.");
            }
            _ => {}
        }
        println!();
    } else {
        let fuzzy = fuzzy_match_command(cmd_name);
        if fuzzy.is_empty() {
            println!("  Unknown command: /{}", cmd_name);
        } else {
            println!("  Unknown command: /{} — did you mean:", cmd_name);
            for cmd in &fuzzy {
                println!("    /{} — {}", cmd.primary, cmd.about);
            }
        }
        println!("  Type /help for available commands.");
    }
}
