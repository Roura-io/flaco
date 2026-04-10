use std::sync::Arc;

use channels::gateway::{ChannelPersona, Gateway, GatewayConfig};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Load tokens from environment
    let app_token = std::env::var("SLACK_APP_TOKEN").unwrap_or_else(|_| {
        eprintln!("Missing SLACK_APP_TOKEN (xapp-...)");
        eprintln!("Set it in ~/.zshrc: export SLACK_APP_TOKEN=\"xapp-...\"");
        std::process::exit(1);
    });

    let bot_token = std::env::var("SLACK_BOT_TOKEN").unwrap_or_else(|_| {
        eprintln!("Missing SLACK_BOT_TOKEN (xoxb-...)");
        eprintln!("Set it in ~/.zshrc: export SLACK_BOT_TOKEN=\"xoxb-...\"");
        std::process::exit(1);
    });

    let ollama_url = std::env::var("OLLAMA_BASE_URL")
        .or_else(|_| std::env::var("OLLAMA_HOST"))
        .ok();
    let model = std::env::var("FLACO_MODEL").ok();

    let gateway_config = GatewayConfig {
        model,
        ollama_url,
        personas: vec![ChannelPersona::slack()],
    };

    let gateway = Arc::new(Gateway::new(gateway_config));

    println!();
    println!("  \x1b[1;36mflacoAi\x1b[0m Slack server (Socket Mode)");
    println!("  Model: {}", gateway.model());
    println!("  Ollama: {}", gateway.ollama_url());
    println!();
    println!("  Connecting to Slack via Socket Mode...");
    println!("  Messages to your bot will be handled automatically.");
    println!("  Press Ctrl+C to stop.");
    println!();

    if let Err(e) = channels::socket_mode::run_socket_mode(&app_token, &bot_token, gateway).await {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}
