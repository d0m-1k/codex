use codex_backend_client::Client as BackendClient;
use codex_core::CodexAuth;
use codex_core::auth::AuthMode;
use codex_core::config::Config;
use codex_protocol::protocol::CreditsSnapshot;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_protocol::protocol::RateLimitWindow;
use codex_utils_cli::CliConfigOverrides;
use serde_json::json;

pub async fn run_status(cli_config_overrides: CliConfigOverrides, json_output: bool) -> ! {
    let config = load_config_or_exit(cli_config_overrides).await;

    let auth = match CodexAuth::from_auth_storage(
        &config.codex_home,
        config.cli_auth_credentials_store_mode,
    ) {
        Ok(Some(auth)) => auth,
        Ok(None) => {
            eprintln!("Not logged in");
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("Error checking login status: {err}");
            std::process::exit(1);
        }
    };

    match auth.auth_mode() {
        AuthMode::ApiKey => {
            if json_output {
                let payload = json!({
                    "auth_mode": "api_key",
                    "rate_limits": [],
                    "message": "Rate limits are only available for ChatGPT login."
                });
                println!("{payload}");
            } else {
                println!("Logged in with API key. Rate limits are only available for ChatGPT login.");
            }
            std::process::exit(0);
        }
        AuthMode::Chatgpt | AuthMode::ChatgptAuthTokens => {}
    }

    let client = match BackendClient::from_auth(config.chatgpt_base_url.clone(), &auth) {
        Ok(client) => client,
        Err(err) => {
            eprintln!("Failed to initialize backend client: {err}");
            std::process::exit(1);
        }
    };

    let snapshots = match client.get_rate_limits_many().await {
        Ok(snapshots) => snapshots,
        Err(err) => {
            eprintln!("Failed to fetch account rate limits: {err}");
            std::process::exit(1);
        }
    };

    if json_output {
        let payload = json!({
            "auth_mode": "chatgpt",
            "rate_limits": snapshots
        });
        match serde_json::to_string_pretty(&payload) {
            Ok(text) => println!("{text}"),
            Err(err) => {
                eprintln!("Failed to serialize status output: {err}");
                std::process::exit(1);
            }
        }
    } else {
        print_human_status(&snapshots);
    }

    std::process::exit(0);
}

async fn load_config_or_exit(cli_config_overrides: CliConfigOverrides) -> Config {
    let cli_overrides = match cli_config_overrides.parse_overrides() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("Error parsing -c overrides: {err}");
            std::process::exit(1);
        }
    };

    match Config::load_with_cli_overrides(cli_overrides).await {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error loading configuration: {err}");
            std::process::exit(1);
        }
    }
}

fn print_human_status(snapshots: &[RateLimitSnapshot]) {
    if snapshots.is_empty() {
        println!("No rate limit data available.");
        return;
    }

    for snapshot in snapshots {
        println!("{}", limit_label(snapshot));
        println!("{}", format_window_line("Primary", snapshot.primary.as_ref()));
        println!(
            "{}",
            format_window_line("Secondary", snapshot.secondary.as_ref())
        );
        println!("{}", format_credits_line(snapshot.credits.as_ref()));
        if let Some(plan_type) = snapshot.plan_type {
            println!("  Plan: {plan_type}");
        }
        println!();
    }
}

fn limit_label(snapshot: &RateLimitSnapshot) -> String {
    let label = snapshot
        .limit_name
        .clone()
        .or_else(|| snapshot.limit_id.clone())
        .unwrap_or_else(|| "default".to_string());
    format!("Limit: {label}")
}

fn format_window_line(label: &str, window: Option<&RateLimitWindow>) -> String {
    match window {
        Some(window) => {
            let remaining = (100.0 - window.used_percent).clamp(0.0, 100.0);
            let window_minutes = window
                .window_minutes
                .map(|minutes| format!("{minutes}m"))
                .unwrap_or_else(|| "unknown".to_string());
            let resets_at = window
                .resets_at
                .map(|timestamp| timestamp.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "  {label}: used={:.1}% remaining={remaining:.1}% window={window_minutes} resets_at_unix={resets_at}",
                window.used_percent
            )
        }
        None => format!("  {label}: unavailable"),
    }
}

fn format_credits_line(credits: Option<&CreditsSnapshot>) -> String {
    match credits {
        Some(credits) => {
            let balance = credits.balance.as_deref().unwrap_or("unknown");
            format!(
                "  Credits: has_credits={} unlimited={} balance={balance}",
                credits.has_credits, credits.unlimited
            )
        }
        None => "  Credits: unavailable".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::format_window_line;
    use super::limit_label;
    use codex_protocol::protocol::RateLimitSnapshot;
    use codex_protocol::protocol::RateLimitWindow;

    #[test]
    fn falls_back_to_limit_id_for_label() {
        let snapshot = RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: None,
            secondary: None,
            credits: None,
            plan_type: None,
        };
        assert_eq!(limit_label(&snapshot), "Limit: codex");
    }

    #[test]
    fn formats_window_values() {
        let line = format_window_line(
            "Primary",
            Some(&RateLimitWindow {
                used_percent: 25.0,
                window_minutes: Some(300),
                resets_at: Some(1_700_000_000),
            }),
        );
        assert_eq!(
            line,
            "  Primary: used=25.0% remaining=75.0% window=300m resets_at_unix=1700000000"
        );
    }
}
