//! Alter-collection diffs: storage, quantization, and vector diff lowering.
//!
//! Pure move from `ddl.rs` (size hygiene split).

use crate::types::*;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use qql_core::ast::{CollectionConfig, QuantizationUpdate, SparseIndexConfig, VectorsConfig};
use qql_core::error::QqlError;

/// Lower `VECTOR (on_disk = …, memory = …)` storage settings into the dense
/// diff. `datatype` has no `VectorParamsDiff` wire field, so it fails closed.
pub(crate) fn lower_storage_diff(storage: &VectorsConfig) -> Result<VectorParamsDiff, QqlError> {
    if storage.datatype.is_some() {
        return Err(vector_diff_error(
            "datatype cannot be changed per vector: the Qdrant VectorParamsDiff wire shape has no datatype field. Recreate the collection or set the datatype at CREATE COLLECTION",
        ));
    }
    Ok(VectorParamsDiff {
        on_disk: storage.on_disk,
        memory: storage.memory,
        ..Default::default()
    })
}

/// Lower a per-vector `QUANTIZATION (…)` replacement: `disabled = true` clears
/// the vector's quantization, otherwise the config replaces it.
pub(crate) fn lower_quantization_update_diff(
    update: Option<&QuantizationUpdate>,
) -> Option<QuantizationConfigDiff> {
    let update = update?;
    if update.disabled {
        return Some(QuantizationConfigDiff::Disabled);
    }
    update.config.as_deref().map(|config| {
        QuantizationConfigDiff::Config(super::runtime::lower_quantization_config(config))
    })
}

/// Insert one dense diff, rejecting a duplicate name (the AST can be built by
/// hand, bypassing the parser's duplicate check).
pub(crate) fn insert_vector_diff(
    map: &mut Option<BTreeMap<String, VectorParamsDiff>>,
    name: String,
    diff: VectorParamsDiff,
) -> Result<(), QqlError> {
    let map = map.get_or_insert_with(BTreeMap::new);
    if map.contains_key(&name) {
        return Err(vector_diff_error(format!(
            "duplicate vector diff for '{}'",
            display_vector_name(&name)
        )));
    }
    map.insert(name, diff);
    Ok(())
}

/// Insert one sparse diff, rejecting a duplicate name.
pub(crate) fn insert_sparse_vector_diff(
    map: &mut Option<BTreeMap<String, SparseVectorParamsDiff>>,
    name: String,
    diff: SparseVectorParamsDiff,
) -> Result<(), QqlError> {
    let map = map.get_or_insert_with(BTreeMap::new);
    if map.contains_key(&name) {
        return Err(vector_diff_error(format!(
            "duplicate sparse vector diff for '{}'",
            display_vector_name(&name)
        )));
    }
    map.insert(name, diff);
    Ok(())
}

fn display_vector_name(name: &str) -> &str {
    if name.is_empty() { "<default>" } else { name }
}

/// `none` is the only non-modifying sparse modifier; every other accepted
/// spelling is `idf`.
pub(crate) fn lower_sparse_modifier(raw: &str) -> SparseModifier {
    if raw.eq_ignore_ascii_case("none") {
        SparseModifier::None
    } else {
        SparseModifier::Idf
    }
}

fn vector_diff_error(message: impl Into<alloc::borrow::Cow<'static, str>>) -> QqlError {
    QqlError::validation("QQL-PLAN-VECTOR-DIFF", message, None)
}

pub(crate) fn lower_sparse_index_params(index: &SparseIndexConfig) -> SparseIndexParams {
    SparseIndexParams {
        full_scan_threshold: index.full_scan_threshold,
        on_disk: index.on_disk,
        memory: index.memory,
        datatype: index.datatype,
    }
}

/// `ALTER COLLECTION` quantization replacement: `disabled` wins, otherwise the
/// replacement config (falling back to a plain `WITH QUANTIZATION` block).
pub(crate) fn lower_quantization_diff(config: &CollectionConfig) -> Option<QuantizationConfigDiff> {
    if let Some(ref update) = config.quantization_update {
        if update.disabled {
            return Some(QuantizationConfigDiff::Disabled);
        }
        return update
            .config
            .as_deref()
            .or(config.quantization.as_deref())
            .map(|q| QuantizationConfigDiff::Config(super::runtime::lower_quantization_config(q)));
    }
    config
        .quantization
        .as_deref()
        .map(|q| QuantizationConfigDiff::Config(super::runtime::lower_quantization_config(q)))
}
