//! `qql` — Qdrant Query Language CLI: query exec, scripts, explain, REPL,
//! REST→QQL conversion, collection dump, cluster migrate, formatting, and
//! edge configuration.
use clap::Parser;
use std::path::PathBuf;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod commands;
mod config;
mod convert;
mod dump;
mod migrate;
mod output;
mod repl;
mod script;
mod table;

#[derive(Parser)]
#[command(name = "qql", about = "Qdrant Query Language CLI", version)]
struct Cli {
    /// Qdrant REST URL. Overrides QDRANT_URL when supplied.
    #[arg(long, global = true)]
    url: Option<String>,
    /// Execute supported commands against the configured in-process edge backend.
    #[arg(long, global = true)]
    edge: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Execute a QQL query
    Exec {
        /// QQL query string (e.g., "QUERY 'hello' FROM docs LIMIT 5")
        query: String,
        /// Parameter in key=value format (can be specified multiple times)
        #[arg(long = "param", short = 'p')]
        params: Vec<String>,
        /// Path to JSON file containing parameter map or positional array
        #[arg(long = "params-file")]
        params_file: Option<PathBuf>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Execute multiple QQL queries from a file
    Execute {
        /// Path to .qql script file
        file: String,
        /// Stop on first error
        #[arg(long)]
        stop_on_error: bool,
    },
    /// Explain a QQL query (show execution plan)
    Explain {
        query: String,
        /// Parameter in key=value format (can be specified multiple times)
        #[arg(long = "param", short = 'p')]
        params: Vec<String>,
        /// Path to JSON file containing parameter map or positional array
        #[arg(long = "params-file")]
        params_file: Option<PathBuf>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Start interactive REPL connected to Qdrant
    #[command(alias = "repl")]
    Connect,
    /// Convert REST JSON payload to QQL
    Convert {
        /// Path to JSON file (or stdin if omitted)
        file: Option<String>,
    },
    /// Format QQL source into canonical form
    Fmt {
        /// Path to .qql file (or stdin if omitted)
        file: Option<String>,
        /// Check formatting without writing; exit non-zero if changes are needed
        #[arg(long)]
        check: bool,
        /// Write the formatted output back to the file
        #[arg(long)]
        write: bool,
    },
    /// Dump collection to .qql file
    Dump {
        collection: String,
        output: String,
        #[arg(long, default_value = "100")]
        batch_size: u32,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Migrate a collection between clusters (schema + points, not snapshots)
    Migrate(Box<MigrateArgs>),
    /// Local qdrant-edge backend utilities (no server required)
    Edge {
        #[command(subcommand)]
        command: Box<EdgeCommand>,
    },
    /// Check Qdrant connection health
    Doctor {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Triage one statement: format, explain, embed probe, topology, doctor
    Check {
        /// QQL query string (e.g., "QUERY 'hello' FROM docs LIMIT 5")
        query: String,
        /// Parameter in key=value format (can be specified multiple times)
        #[arg(long = "param", short = 'p')]
        params: Vec<String>,
        /// Path to JSON file containing parameter map or positional array
        #[arg(long = "params-file")]
        params_file: Option<PathBuf>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Configure persistent CLI settings.
    Config {
        #[command(subcommand)]
        command: Box<ConfigCommand>,
    },
    /// Show version
    Version,
}

#[derive(clap::Args)]
struct MigrateArgs {
    /// Source collection
    collection: String,
    /// Target collection name (defaults to the source name)
    #[arg(long = "to")]
    to: Option<String>,
    /// Target Qdrant URL (defaults to --url)
    #[arg(long)]
    target_url: Option<String>,
    /// Use the local edge backend as the target
    #[arg(long)]
    target_edge: bool,
    /// Use the local edge backend as the source (also implied by the global --edge flag)
    #[arg(long)]
    source_edge: bool,
    /// API key for the target cluster
    #[arg(long, env = "QDRANT_TARGET_API_KEY")]
    target_api_key: Option<String>,
    /// Scroll / upsert batch size
    #[arg(long, default_value_t = migrate::DEFAULT_BATCH_SIZE)]
    batch_size: u32,
    /// Concurrent upsert streams
    #[arg(long, default_value_t = migrate::DEFAULT_WORKERS)]
    workers: usize,
    /// Override target shard_number
    #[arg(long)]
    shard_number: Option<u64>,
    /// Override target replication_factor
    #[arg(long)]
    replication_factor: Option<u64>,
    /// Override sharding method (`auto` or `custom`)
    #[arg(long)]
    sharding_method: Option<String>,
    /// Apply quantization on CREATE (scalar, binary, product, turbo)
    #[arg(long, value_enum)]
    quantize: Option<CliQuantize>,
    /// Store quantized vectors on disk instead of RAM
    #[arg(long)]
    no_always_ram: bool,
    /// Scalar quantization quantile
    #[arg(long, default_value_t = 0.99)]
    quantize_quantile: f64,
    /// Product quantization compression (`x4`/`x8`/`x16`/`x32`)
    #[arg(long, default_value = "x16")]
    quantize_compression: String,
    /// Binary quantization encoding
    #[arg(long, default_value = "one_bit")]
    quantize_encoding: String,
    /// Turbo quantization bits (`1`/`1.5`/`2`/`4`)
    #[arg(long, default_value = "2")]
    quantize_bits: String,
    /// Fixed custom shard key for every upsert
    #[arg(long)]
    shard_key: Option<String>,
    /// Payload field used as the per-point custom shard key
    #[arg(long)]
    shard_key_field: Option<String>,
    /// Missing `--shard-key-field` policy: error, skip, or `default=<key>`
    #[arg(long, default_value = "error")]
    on_missing_shard_key: String,
    /// Optimizer indexing_threshold (KB) during bulk load
    #[arg(long, default_value_t = migrate::DEFAULT_BULK_INDEXING_THRESHOLD)]
    bulk_threshold_kb: u64,
    /// After verify, atomically point this alias at the target collection
    #[arg(long = "cutover")]
    cutover: Option<String>,
    /// After a successful cutover, DROP the source collection
    #[arg(long)]
    drop_source_after_cutover: bool,
    /// Restrict the source scroll (`city = 'berlin'`)
    #[arg(long = "where")]
    where_clause: Option<String>,
    /// Checkpoint file (default `.qql-migrate/<src>__<dst>.json`)
    #[arg(long)]
    checkpoint: Option<String>,
    /// Resume from an existing checkpoint
    #[arg(long)]
    resume: bool,
    /// Ignore any existing checkpoint and start over
    #[arg(long)]
    restart: bool,
    /// Print the plan and exit without writing
    #[arg(long)]
    dry_run: bool,
    /// Do not suppress HNSW during ingest
    #[arg(long)]
    no_fast_bulk: bool,
    /// Skip exact count verification
    #[arg(long)]
    no_verify: bool,
    /// Skip WAIT true on upserts (faster, weaker durability)
    #[arg(long)]
    no_wait: bool,
    /// DROP the target collection before creating it
    #[arg(long)]
    recreate: bool,
    /// Output as JSON
    #[arg(long)]
    json: bool,
    /// Quiet mode
    #[arg(long, short)]
    quiet: bool,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum CliQuantize {
    Scalar,
    Binary,
    Product,
    Turbo,
}

impl From<CliQuantize> for migrate::QuantizeKind {
    fn from(value: CliQuantize) -> Self {
        match value {
            CliQuantize::Scalar => Self::Scalar,
            CliQuantize::Binary => Self::Binary,
            CliQuantize::Product => Self::Product,
            CliQuantize::Turbo => Self::Turbo,
        }
    }
}

#[derive(clap::Subcommand)]
enum ConfigCommand {
    /// Configure the local qdrant-edge backend used by --edge.
    Edge {
        /// Directory for persistent qdrant-edge data.
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Keep payloads in memory instead of persisting them to disk.
        #[arg(long)]
        in_memory: bool,
        /// WAL segment capacity in MiB for local edge shards (default: qdrant-edge 32 MiB).
        #[arg(long)]
        wal_segment_mb: Option<u64>,
        /// Embedding backend: fastembed or an OpenAI-compatible HTTP endpoint.
        #[arg(long, default_value = "fastembed")]
        embedder: String,
        /// Local FastEmbed dense model name or alias.
        #[arg(long)]
        model: Option<String>,
        /// Offline sparse model for fastembed (e.g. splade, bge-m3).
        #[arg(long)]
        sparse_model: Option<String>,
        /// Offline multivector model for fastembed (e.g. bge-m3).
        #[arg(long)]
        multi_model: Option<String>,
        /// Offline CLIP vision model for fastembed (e.g. clip-vision).
        #[arg(long)]
        image_model: Option<String>,
        /// Offline cross-encoder model (e.g. bge-reranker-base).
        #[arg(long)]
        reranker_model: Option<String>,
        /// Directory used for downloaded FastEmbed models.
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        /// Show model download progress.
        #[arg(long)]
        show_download_progress: bool,
        /// OpenAI-compatible embedding endpoint used by the HTTP backend.
        #[arg(long)]
        embed_url: Option<String>,
        /// API key used by the HTTP embedding backend.
        #[arg(long, default_value = "")]
        embed_key: String,
        /// Model name sent to the HTTP embedding backend.
        #[arg(long, default_value = "nomic-embed-text")]
        embed_model: String,
        /// Expected HTTP embedding dimension.
        #[arg(long, default_value_t = 768)]
        embed_dim: usize,
        /// Optional multi/ColBERT HTTP embedding endpoint.
        #[arg(long)]
        multi_embed_url: Option<String>,
        /// API key for the multi embedding endpoint.
        #[arg(long)]
        multi_embed_key: Option<String>,
        /// Multi/ColBERT model name for HTTP multi embeds.
        #[arg(long)]
        multi_embed_model: Option<String>,
        /// Per-token dimension for multi embeds (0 = skip check).
        #[arg(long, default_value_t = 0)]
        multi_embed_dim: usize,
        /// Optional image/CLIP vision HTTP embedding endpoint.
        #[arg(long)]
        image_embed_url: Option<String>,
        /// API key for the image embedding endpoint.
        #[arg(long)]
        image_embed_key: Option<String>,
        /// Image/CLIP vision model name for HTTP image embeds.
        #[arg(long)]
        image_embed_model: Option<String>,
        /// Dense dimension for image embeds (CLIP = 512; 0 = use dense dim).
        #[arg(long, default_value_t = 0)]
        image_embed_dim: usize,
    },
}

#[derive(clap::Subcommand)]
enum EdgeCommand {
    /// Run qdrant-edge storage optimizers on a local collection
    ///
    /// qdrant-edge has no background optimizer: segments are merged and HNSW /
    /// sparse indexes are built only when this runs. Run it after bulk writes
    /// or when `qql doctor --edge` reports indexing lag.
    Optimize {
        /// Local edge collection to optimize
        collection: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
    /// Seed a local edge collection from a remote Qdrant shard snapshot
    ///
    /// Streams the remote shard snapshot, unpacks it with the engine's snapshot
    /// API, verifies it, and swaps it into the local edge data directory. The
    /// snapshot carries the source collection's config, built HNSW indexes and
    /// quantized data, so nothing is re-indexed locally. An existing local
    /// collection is only replaced with --force.
    Bootstrap {
        /// Collection name (the local edge collection gets the same name)
        collection: String,
        /// Remote Qdrant base URL (defaults to --url / QDRANT_URL)
        #[arg(long = "from")]
        from: Option<String>,
        /// Remote API key (defaults to QDRANT_API_KEY)
        #[arg(long)]
        api_key: Option<String>,
        /// Remote shard id (default: the only shard of a single-shard collection)
        #[arg(long)]
        shard_id: Option<u32>,
        /// Replace an existing local collection directory
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Quiet mode
        #[arg(long, short)]
        quiet: bool,
    },
}

/// Named/positional params for `qql exec`. Bound on the AST (not string-spliced)
/// so `UPSERT … VALUES :rows` can take a JSON array of point objects.
fn collect_exec_params(
    params: &[String],
    params_file: Option<&PathBuf>,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    if params.is_empty() && params_file.is_none() {
        return Ok(None);
    }
    let mut map = serde_json::Map::new();
    if let Some(file_path) = params_file {
        let content = std::fs::read_to_string(file_path)?;
        let parsed: serde_json::Value = serde_json::from_str(&content)?;
        match parsed {
            serde_json::Value::Object(obj) => {
                for (k, v) in obj {
                    let key = k.strip_prefix(':').unwrap_or(&k).to_string();
                    map.insert(key, v);
                }
            }
            serde_json::Value::Array(_) if params.is_empty() => return Ok(Some(parsed)),
            serde_json::Value::Array(_) => {
                return Err("--params-file array cannot be combined with --param key=value".into());
            }
            _ => return Err("--params-file must contain a JSON object or array".into()),
        }
    }
    for p in params {
        let (key_raw, val_raw) = p
            .split_once('=')
            .ok_or_else(|| format!("parameter must be in key=value format, got '{p}'"))?;
        let key = key_raw
            .trim()
            .strip_prefix(':')
            .unwrap_or(key_raw.trim())
            .to_string();
        let val_trimmed = val_raw.trim();
        let parsed_val: serde_json::Value = serde_json::from_str(val_trimmed)
            .unwrap_or_else(|_| serde_json::Value::String(val_trimmed.to_string()));
        map.insert(key, parsed_val);
    }
    Ok(Some(serde_json::Value::Object(map)))
}

fn print_migrate_result(
    source: &str,
    target: &str,
    stats: &migrate::MigrateStats,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "operation": "migrate",
                "source": source,
                "target": target,
                "written": stats.written,
                "skipped": stats.skipped,
                "batches": stats.batches,
                "source_count": stats.source_count,
                "target_count": stats.target_count,
                "verified": stats.verified,
                "resumed": stats.resumed,
                "dry_run": stats.dry_run,
                "cutover_alias": stats.cutover_alias,
                "source_dropped": stats.source_dropped,
                "create": stats.plan.create,
                "indexes": stats.plan.indexes,
                "shard_keys": stats.plan.shard_keys,
                "restore_optimizers": stats.plan.restore_optimizers,
            })
        );
        return Ok(());
    }
    if stats.dry_run {
        println!(
            "Dry-run migrate '{source}' → '{target}' ({} source points)",
            stats.source_count
        );
        println!("{};", stats.plan.create);
        for idx in &stats.plan.indexes {
            println!("{};", idx);
        }
        for key in &stats.plan.shard_keys {
            println!("{};", key);
        }
        if let Some(restore) = &stats.plan.restore_optimizers {
            println!("-- after ingest:");
            println!("{};", restore);
        }
        return Ok(());
    }
    let verified = if stats.verified {
        "verified"
    } else {
        "unverified"
    };
    let resumed = if stats.resumed { ", resumed" } else { "" };
    println!(
        "Migrated '{source}' → '{target}' ({} written, {} skipped, {} batches, {verified}{resumed})",
        stats.written, stats.skipped, stats.batches
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let use_edge = cli.edge;
    let url = cli
        .url
        .or_else(|| std::env::var("QDRANT_URL").ok())
        .unwrap_or_else(|| "http://localhost:6333".to_string());

    match cli.command.unwrap_or(Command::Connect) {
        Command::Exec {
            query,
            params,
            params_file,
            json,
            quiet,
        } => {
            let exec_params = collect_exec_params(&params, params_file.as_ref())?;
            commands::handle_exec(&url, use_edge, &query, exec_params.as_ref(), json, quiet).await
        }
        Command::Execute {
            file,
            stop_on_error,
        } => commands::handle_execute_file(&url, use_edge, &file, stop_on_error).await,
        Command::Explain {
            query,
            params,
            params_file,
            json,
            quiet,
        } => {
            let exec_params = collect_exec_params(&params, params_file.as_ref())?;
            commands::handle_explain(&query, exec_params.as_ref(), json, quiet)
        }
        Command::Connect => commands::handle_connect(&url, use_edge).await,
        Command::Convert { file } => commands::handle_convert(file.as_deref()),
        Command::Fmt { file, check, write } => commands::handle_fmt(file.as_deref(), check, write),
        Command::Dump {
            collection,
            output,
            batch_size,
            json,
            quiet,
        } => {
            use std::io::Write;
            let progress_fn = |p: dump::DumpProgress| {
                eprint!("\rDumped {} points ({} batches)...", p.written, p.batches);
                let _ = std::io::stderr().flush();
            };
            let progress_cb: Option<&(dyn Fn(dump::DumpProgress) + Sync)> = if !json && !quiet {
                Some(&progress_fn)
            } else {
                None
            };
            let stats = commands::handle_dump(
                &url,
                use_edge,
                &collection,
                &output,
                batch_size,
                progress_cb,
            )
            .await?;
            if !json && !quiet && stats.batches > 0 {
                eprintln!();
            }
            let msg = format!(
                "Dumped collection '{}' to {} ({} written, {} skipped, {} batches)",
                collection, output, stats.written, stats.skipped, stats.batches
            );
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": true,
                        "operation": "dump",
                        "collection": collection,
                        "output": output,
                        "written": stats.written,
                        "skipped": stats.skipped,
                        "batches": stats.batches,
                        "message": msg,
                    })
                );
            } else {
                println!("{}", msg);
            }
            Ok(())
        }
        Command::Migrate(args) => {
            use std::io::Write;
            let args = *args;
            let target_collection = args.to.unwrap_or_else(|| args.collection.clone());
            let target_url = args.target_url.unwrap_or_else(|| url.clone());
            let checkpoint = args.checkpoint.unwrap_or_else(|| {
                migrate::default_checkpoint_path(
                    &url,
                    &args.collection,
                    &target_url,
                    &target_collection,
                )
            });
            let quantize = args.quantize.map(|kind| {
                let mut spec = migrate::QuantizeSpec::new(kind.into());
                spec.always_ram = !args.no_always_ram;
                spec.quantile = args.quantize_quantile;
                spec.compression = args.quantize_compression;
                spec.encoding = args.quantize_encoding;
                spec.bits = args.quantize_bits;
                spec
            });
            let opts = migrate::MigrateOptions {
                source_collection: args.collection.clone(),
                target_collection: target_collection.clone(),
                source_url: url.clone(),
                target_url: target_url.clone(),
                batch_size: args.batch_size,
                workers: args.workers,
                shard_number: args.shard_number,
                replication_factor: args.replication_factor,
                sharding_method: args.sharding_method,
                quantize,
                shard_key: args.shard_key,
                shard_key_field: args.shard_key_field,
                missing_shard_key: migrate::MissingShardKey::parse(&args.on_missing_shard_key)
                    .map_err(|e| format!("--on-missing-shard-key: {e}"))?,
                bulk_indexing_threshold: args.bulk_threshold_kb,
                cutover_alias: args.cutover,
                drop_source_after_cutover: args.drop_source_after_cutover,
                where_clause: args.where_clause,
                checkpoint_path: checkpoint,
                resume: args.resume,
                restart: args.restart,
                dry_run: args.dry_run,
                fast_bulk: !args.no_fast_bulk,
                verify: !args.no_verify,
                wait: !args.no_wait,
                recreate: args.recreate,
            };
            let progress_fn = |p: migrate::MigrateProgress| {
                eprint!(
                    "\r[{}] {} — {} / {} points ({} batches)...",
                    p.phase, p.collection, p.written, p.source_count, p.batches
                );
                let _ = std::io::stderr().flush();
            };
            let progress_cb: Option<&(dyn Fn(migrate::MigrateProgress) + Sync)> =
                if !args.json && !args.quiet && !args.dry_run {
                    Some(&progress_fn)
                } else {
                    None
                };
            let stats = commands::handle_migrate(
                &url,
                use_edge || args.source_edge,
                &target_url,
                args.target_edge,
                args.target_api_key,
                opts,
                progress_cb,
            )
            .await?;
            if !args.json && !args.quiet && stats.batches > 0 {
                eprintln!();
            }
            print_migrate_result(&args.collection, &target_collection, &stats, args.json)?;
            Ok(())
        }
        Command::Edge { command } => match *command {
            EdgeCommand::Optimize {
                collection,
                json,
                quiet,
            } => {
                #[cfg(feature = "edge")]
                {
                    commands::handle_edge_optimize(&collection, json, quiet).await
                }
                #[cfg(not(feature = "edge"))]
                {
                    let _ = (collection, json, quiet);
                    Err(
                        "edge support is not installed; reinstall qql-cli with --features edge"
                            .into(),
                    )
                }
            }
            EdgeCommand::Bootstrap {
                collection,
                from,
                api_key,
                shard_id,
                force,
                json,
                quiet,
            } => {
                #[cfg(feature = "edge")]
                {
                    let from = from.unwrap_or_else(|| url.clone());
                    commands::handle_edge_bootstrap(
                        &from,
                        api_key,
                        &collection,
                        shard_id,
                        force,
                        json,
                        quiet,
                    )
                    .await
                }
                #[cfg(not(feature = "edge"))]
                {
                    let _ = (collection, from, api_key, shard_id, force, json, quiet);
                    Err(
                        "edge support is not installed; reinstall qql-cli with --features edge"
                            .into(),
                    )
                }
            }
        },
        Command::Doctor { json, quiet } => {
            commands::handle_doctor(&url, use_edge, json, quiet).await
        }
        Command::Check {
            query,
            params,
            params_file,
            json,
            quiet,
        } => {
            let check_params = collect_exec_params(&params, params_file.as_ref())?;
            commands::handle_check(&url, use_edge, &query, check_params.as_ref(), json, quiet).await
        }
        Command::Config { command } => match *command {
            ConfigCommand::Edge {
                data_dir,
                in_memory,
                wal_segment_mb,
                embedder,
                model,
                sparse_model,
                multi_model,
                image_model,
                reranker_model,
                cache_dir,
                show_download_progress,
                embed_url,
                embed_key,
                embed_model,
                embed_dim,
                multi_embed_url,
                multi_embed_key,
                multi_embed_model,
                multi_embed_dim,
                image_embed_url,
                image_embed_key,
                image_embed_model,
                image_embed_dim,
            } => commands::handle_configure_edge(config::EdgeConfig {
                data_dir: data_dir.unwrap_or_else(|| config::EdgeConfig::default().data_dir),
                on_disk_payload: !in_memory,
                wal_segment_mb,
                embedder,
                model,
                sparse_model,
                multi_model,
                image_model,
                reranker_model,
                cache_dir,
                show_download_progress,
                embed_url,
                embed_key,
                embed_model,
                embed_dimension: embed_dim,
                multi_embed_url,
                multi_embed_key,
                multi_embed_model,
                multi_embed_dimension: multi_embed_dim,
                image_embed_url,
                image_embed_key,
                image_embed_model,
                image_embed_dimension: image_embed_dim,
            }),
        },
        Command::Version => commands::handle_version(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_exec_params_named() {
        let params = vec![
            "q=laptop".to_string(),
            ":p=999.50".to_string(),
            "l=5".to_string(),
        ];
        let out = collect_exec_params(&params, None).unwrap().unwrap();
        assert_eq!(out["q"], serde_json::json!("laptop"));
        assert_eq!(out["p"], serde_json::json!(999.5));
        assert_eq!(out["l"], serde_json::json!(5));
    }

    #[test]
    fn test_collect_exec_params_empty() {
        assert!(collect_exec_params(&[], None).unwrap().is_none());
    }

    #[test]
    fn test_explain_binds_rows_params() {
        // Whole-point placeholders bind on the AST, so explain accepts the
        // same `:rows` batch files as exec (no string splicing).
        let params = serde_json::json!({"rows": [{"id": 1}]});
        commands::handle_explain(
            "UPSERT INTO docs VALUES :rows WAIT true;",
            Some(&params),
            true,
            true,
        )
        .unwrap();
    }

    #[cfg(feature = "edge")]
    #[test]
    fn wal_segment_capacity_scales_mib_and_rejects_zero() {
        assert_eq!(commands::wal_segment_capacity_bytes(None).unwrap(), None);
        assert!(commands::wal_segment_capacity_bytes(Some(0)).is_err());
        assert_eq!(
            commands::wal_segment_capacity_bytes(Some(4)).unwrap(),
            Some(4 * 1024 * 1024)
        );
    }
}
