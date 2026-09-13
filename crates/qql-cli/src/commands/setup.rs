//! `qql setup` and `qql config [show|get|set|path]` handlers.

use qql::config::QqlConfig;
use std::io::{self, IsTerminal, Write};

#[derive(Debug, Default)]
pub struct SetupOptions {
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub embed_url: Option<String>,
    pub embed_model: Option<String>,
    pub embed_dim: Option<usize>,
    pub edge: bool,
    pub non_interactive: bool,
}

pub async fn handle_setup(opts: SetupOptions) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = QqlConfig::load()?.unwrap_or_default();
    let is_tty = io::stdin().is_terminal();

    if !opts.non_interactive && is_tty && opts.url.is_none() {
        println!("\x1b[1m\x1b[36mQQL Setup Wizard\x1b[0m");
        println!("Configure your connection to Qdrant and local/remote embedding models.\n");

        // 1. Qdrant URL
        let default_url = if !config.url.is_empty() {
            &config.url
        } else {
            "http://localhost:6333"
        };
        print!("Qdrant endpoint URL [\x1b[2m{}\x1b[0m]: ", default_url);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let chosen_url = input.trim();
        config.url = if chosen_url.is_empty() {
            default_url.to_string()
        } else {
            chosen_url.to_string()
        };

        // 2. Qdrant API Key
        let has_secret = config.secret.is_some();
        let prompt_secret = if has_secret {
            " (leave empty to keep existing)"
        } else {
            " (optional)"
        };
        print!("Qdrant API key{}: ", prompt_secret);
        io::stdout().flush()?;
        let mut secret_input = String::new();
        io::stdin().read_line(&mut secret_input)?;
        let trimmed_secret = secret_input.trim();
        if !trimmed_secret.is_empty() {
            config.secret = Some(trimmed_secret.to_string());
        }

        // 3. Embedding Endpoint
        let default_embed = config.embedding_endpoint.as_deref().unwrap_or("none");
        print!(
            "OpenAI-compatible embedder endpoint (e.g. Ollama http://localhost:11434/v1/embeddings) [\x1b[2m{}\x1b[0m]: ",
            default_embed
        );
        io::stdout().flush()?;
        let mut embed_input = String::new();
        io::stdin().read_line(&mut embed_input)?;
        let trimmed_embed = embed_input.trim();
        if !trimmed_embed.is_empty() {
            if trimmed_embed.eq_ignore_ascii_case("none") {
                config.embedding_endpoint = None;
            } else {
                config.embedding_endpoint = Some(trimmed_embed.to_string());
            }
        }

        // 4. Embedding Model and Dim
        if config.embedding_endpoint.is_some() {
            let default_model = config
                .embedding_model
                .as_deref()
                .unwrap_or("all-minilm:l6-v2");
            print!("Embedding model [\x1b[2m{}\x1b[0m]: ", default_model);
            io::stdout().flush()?;
            let mut model_input = String::new();
            io::stdin().read_line(&mut model_input)?;
            let trimmed_model = model_input.trim();
            config.embedding_model = Some(if trimmed_model.is_empty() {
                default_model.to_string()
            } else {
                trimmed_model.to_string()
            });

            let default_dim = if config.embedding_dimension > 0 {
                config.embedding_dimension
            } else {
                384
            };
            print!("Embedding dimension [\x1b[2m{}\x1b[0m]: ", default_dim);
            io::stdout().flush()?;
            let mut dim_input = String::new();
            io::stdin().read_line(&mut dim_input)?;
            let trimmed_dim = dim_input.trim();
            if let Ok(d) = trimmed_dim.parse::<usize>() {
                config.embedding_dimension = d;
            } else {
                config.embedding_dimension = default_dim;
            }
        }
    } else {
        // Non-interactive or flags supplied
        if let Some(url) = opts.url {
            config.url = url;
        } else if config.url.is_empty() {
            config.url = "http://localhost:6333".to_string();
        }
        if let Some(key) = opts.api_key {
            config.secret = Some(key);
        }
        if let Some(embed_url) = opts.embed_url {
            config.embedding_endpoint = Some(embed_url);
        }
        if let Some(model) = opts.embed_model {
            config.embedding_model = Some(model);
        }
        if let Some(dim) = opts.embed_dim {
            config.embedding_dimension = dim;
        }
    }

    // Save the configuration
    config.save()?;
    let path = QqlConfig::config_path()?;

    println!(
        "\n\x1b[32m✓\x1b[0m Configuration saved to \x1b[1m{}\x1b[0m",
        path.display()
    );

    // Test connectivity
    print!("Testing connection to {}... ", config.url);
    io::stdout().flush()?;
    match super::runtime::executor_for(&config.url, opts.edge, config.secret.clone()) {
        Ok(exec) => {
            match exec
                .execute("SHOW COLLECTIONS", qql::executor::OnError::Stop)
                .await
            {
                Ok(_) => {
                    println!("\x1b[32mOK (reachable)\x1b[0m");
                }
                Err(e) => {
                    println!("\x1b[33mWarning: {}\x1b[0m", e);
                    println!("  \x1b[2m(Configuration was saved; start Qdrant to connect)\x1b[0m");
                }
            }
        }
        Err(e) => {
            println!("\x1b[33mWarning: {}\x1b[0m", e);
        }
    }

    println!("\nYou are ready! Try:");
    println!("  • \x1b[36mqql repl\x1b[0m              (interactive shell)");
    println!("  • \x1b[36mqql doctor\x1b[0m            (full connectivity & embedder diagnostics)");
    println!("  • \x1b[36mqql exec \"SHOW COLLECTIONS\"\x1b[0m");
    Ok(())
}

pub fn handle_config_show(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = QqlConfig::load()?.unwrap_or_default();
    let path = QqlConfig::config_path()?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": path.display().to_string(),
                "config": config,
            }))?
        );
        return Ok(());
    }

    println!("\x1b[1mQQL Configuration\x1b[0m ({})", path.display());
    let url_src = if std::env::var("QDRANT_URL").is_ok() {
        " (overridden by QDRANT_URL)"
    } else {
        ""
    };
    let effective_url = if let Ok(u) = std::env::var("QDRANT_URL") {
        u
    } else if !config.url.is_empty() {
        config.url.clone()
    } else {
        "http://localhost:6333".to_string()
    };
    println!(
        "  • URL:                 \x1b[36m{}\x1b[0m{}",
        effective_url, url_src
    );

    let key_label = if std::env::var("QDRANT_API_KEY").is_ok() {
        "set (from QDRANT_API_KEY)"
    } else if config.secret.is_some() {
        "set (persisted)"
    } else {
        "unset"
    };
    println!("  • API Key:             {}", key_label);

    let embed_url = config.embedding_endpoint.as_deref().unwrap_or("none");
    println!("  • Embed Endpoint:      {}", embed_url);
    if config.embedding_endpoint.is_some() {
        println!(
            "  • Embed Model:         {}",
            config
                .embedding_model
                .as_deref()
                .unwrap_or("all-minilm:l6-v2")
        );
        println!(
            "  • Embed Dimension:     {}",
            if config.embedding_dimension > 0 {
                config.embedding_dimension
            } else {
                384
            }
        );
    }
    if let Some(re) = &config.rerank_endpoint {
        println!("  • Rerank Endpoint:     {}", re);
    }
    println!(
        "\nTo edit, run \x1b[36mqql setup\x1b[0m or \x1b[36mqql config set <key> <value>\x1b[0m"
    );
    Ok(())
}

pub fn handle_config_get(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = QqlConfig::load()?.unwrap_or_default();
    let normalized = key.to_lowercase().replace('_', "-");
    let val = match normalized.as_str() {
        "url" => config.url,
        "api-key" | "secret" | "key" => config.secret.unwrap_or_default(),
        "embed-url" | "embedding-endpoint" => config.embedding_endpoint.unwrap_or_default(),
        "embed-model" | "embedding-model" => config.embedding_model.unwrap_or_default(),
        "embed-dim" | "embedding-dimension" => config.embedding_dimension.to_string(),
        "rerank-endpoint" => config.rerank_endpoint.unwrap_or_default(),
        "rerank-model" => config.rerank_model.unwrap_or_default(),
        _ => return Err(format!("unknown configuration key '{}'", key).into()),
    };
    println!("{}", val);
    Ok(())
}

pub fn handle_config_set(key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = QqlConfig::load()?.unwrap_or_default();
    let normalized = key.to_lowercase().replace('_', "-");
    match normalized.as_str() {
        "url" => config.url = value.to_string(),
        "api-key" | "secret" | "key" => {
            if value.is_empty() || value == "none" {
                config.secret = None;
            } else {
                config.secret = Some(value.to_string());
            }
        }
        "embed-url" | "embedding-endpoint" => {
            if value.is_empty() || value == "none" {
                config.embedding_endpoint = None;
            } else {
                config.embedding_endpoint = Some(value.to_string());
            }
        }
        "embed-model" | "embedding-model" => {
            config.embedding_model = Some(value.to_string());
        }
        "embed-dim" | "embedding-dimension" => {
            config.embedding_dimension = value.parse::<usize>()
                .map_err(|_| format!("'{}' is not a valid dimension integer", value))?;
        }
        "rerank-endpoint" => {
            if value.is_empty() || value == "none" {
                config.rerank_endpoint = None;
            } else {
                config.rerank_endpoint = Some(value.to_string());
            }
        }
        "rerank-model" => {
            config.rerank_model = Some(value.to_string());
        }
        _ => return Err(format!("unknown configuration key '{}'; supported keys: url, api-key, embed-url, embed-model, embed-dim, rerank-endpoint, rerank-model", key).into()),
    }

    config.save()?;
    println!("\x1b[32m✓\x1b[0m Set \x1b[1m{}\x1b[0m = {}", key, value);
    Ok(())
}

pub fn handle_config_path() -> Result<(), Box<dyn std::error::Error>> {
    let path = QqlConfig::config_path()?;
    println!("{}", path.display());
    Ok(())
}
