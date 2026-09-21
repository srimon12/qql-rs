//! Quota lowering: SET QUOTA statements to the typed quota request.
//!
//! Pure move from `ddl.rs` (size hygiene split).

use crate::types::*;
use alloc::format;
use qql_core::ast::{SetQuotaStmt, Value};
use qql_core::error::QqlError;

/// Lower a `SET QUOTA (…)` statement to a typed quota request.
///
/// `PUT /quotas` **replaces** the entire cluster-wide config. Omitted keys
/// (including `key = null`) are not set on the replacement body, so they
/// become "uncapped / default" in the new config — not a patch of the old
/// one. Callers that want to keep existing limits must restate them.
pub(crate) fn lower_set_quota(stmt: &SetQuotaStmt) -> Result<SetQuotaRequest, QqlError> {
    let mut request = SetQuotaRequest {
        config: QuotaConfig::default(),
        wait: stmt.wait,
    };
    for (key, value) in &stmt.config {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "enabled" => match value {
                Value::Bool(b) => request.config.enabled = Some(*b),
                _ => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-QUOTA",
                        "enabled must be true or false",
                        value.param_span(),
                    ));
                }
            },
            "max_resident_memory_percent" | "max_disk_usage_percent" => {
                request = apply_quota_percent(request, &lower, value, 1, 100)?;
            }
            "release_margin_percent" => {
                request = apply_quota_percent(request, &lower, value, 0, 100)?;
            }
            _ => {
                return Err(QqlError::validation(
                    "QQL-PLAN-QUOTA",
                    format!(
                        "unknown quota parameter '{key}'. Expected: enabled, max_resident_memory_percent, max_disk_usage_percent, release_margin_percent"
                    ),
                    value.param_span(),
                ));
            }
        }
    }
    Ok(request)
}

/// Owned variant of [`lower_set_quota`]: moves config keys.
pub(crate) fn lower_set_quota_owned(stmt: SetQuotaStmt) -> Result<SetQuotaRequest, QqlError> {
    let mut request = SetQuotaRequest {
        config: QuotaConfig::default(),
        wait: stmt.wait,
    };
    for (key, value) in &stmt.config {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "enabled" => match value {
                Value::Bool(b) => request.config.enabled = Some(*b),
                _ => {
                    return Err(QqlError::validation(
                        "QQL-PLAN-QUOTA",
                        "enabled must be true or false",
                        value.param_span(),
                    ));
                }
            },
            "max_resident_memory_percent" | "max_disk_usage_percent" => {
                request = apply_quota_percent(request, &lower, value, 1, 100)?;
            }
            "release_margin_percent" => {
                request = apply_quota_percent(request, &lower, value, 0, 100)?;
            }
            _ => {
                return Err(QqlError::validation(
                    "QQL-PLAN-QUOTA",
                    format!(
                        "unknown quota parameter '{key}'. Expected: enabled, max_resident_memory_percent, max_disk_usage_percent, release_margin_percent"
                    ),
                    value.param_span(),
                ));
            }
        }
    }
    Ok(request)
}

fn apply_quota_percent(
    mut request: SetQuotaRequest,
    key: &str,
    value: &Value,
    min: u64,
    max: u64,
) -> Result<SetQuotaRequest, QqlError> {
    match value {
        // Explicit null → leave field unset so the replacement config has no
        // cap for this resource (full PUT replace semantics).
        Value::Null => {}
        Value::Int(n) if *n >= min as i64 && (*n as u64) <= max => {
            let n = *n as u64;
            match key {
                "max_resident_memory_percent" => {
                    request.config.max_resident_memory_percent = Some(n)
                }
                "max_disk_usage_percent" => request.config.max_disk_usage_percent = Some(n),
                _ => request.config.release_margin_percent = Some(n),
            }
        }
        _ => {
            return Err(QqlError::validation(
                "QQL-PLAN-QUOTA",
                format!("{key} must be an integer in [{min}, {max}] or null"),
                value.param_span(),
            ));
        }
    }
    Ok(request)
}
