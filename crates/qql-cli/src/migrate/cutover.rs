//! Zero-downtime alias cutover after a successful migrate.

use std::error::Error;

use qql::client::AliasAction;
use qql::executor::Executor;

use super::options::MigrateOptions;
use super::schema::run_sql;
use crate::dump::format_ident;

/// Point `alias` at the target collection. Deletes any existing alias first
/// so the swap is a single `change_aliases` batch when the name is in use.
pub async fn cutover_alias(
    target: &Executor,
    opts: &MigrateOptions,
    alias: &str,
) -> Result<(), Box<dyn Error>> {
    let actions = vec![
        AliasAction::Delete {
            alias: alias.to_string(),
        },
        AliasAction::Create {
            collection: opts.target_collection.clone(),
            alias: alias.to_string(),
        },
    ];
    match target.ops().change_aliases(&actions).await {
        Ok(()) => Ok(()),
        Err(err) if alias_missing(&err.to_string()) => target
            .ops()
            .change_aliases(&[AliasAction::Create {
                collection: opts.target_collection.clone(),
                alias: alias.to_string(),
            }])
            .await
            .map_err(|e| e.into()),
        Err(err) => Err(err.into()),
    }
}

/// Drop the source collection after a successful cutover.
pub async fn drop_source(source: &Executor, collection: &str) -> Result<(), Box<dyn Error>> {
    run_sql(
        source,
        &format!("DROP COLLECTION {};", format_ident(collection)),
    )
    .await
}

fn alias_missing(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("not found")
        || lower.contains("doesn't exist")
        || lower.contains("does not exist")
}
