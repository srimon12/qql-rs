"""ExecResponse -> rows + ``description`` mapping for :mod:`pyqql._dbapi`.

Each statement result ``{operation, message, data}`` maps to ``(rows,
description)`` where rows are plain tuples aligned with ``description``
(DB-API 7-tuples). Point hits map to ``(id, score, payload[, vector])``
with payload always a dict; ``COUNT`` to ``[(n,)]``; ``FACET`` to
``(value, count)`` rows; ``QUERY_GROUPS`` to ``(group_id, hits)`` rows;
``SHOW_COLLECTIONS`` to ``(name,)`` rows. Anything else (DML, DDL,
SHOW variants without a tabular shape) yields ``([], None)`` and the
caller derives ``rowcount`` (-1, or the UPSERT affected count).
"""

from typing import Any, Dict, List, Optional, Tuple

__all__ = ["POINT_OPS", "map_result"]

POINT_OPS = frozenset({"QUERY", "GET_POINTS", "SCROLL", "CROSS_RERANK"})


def _col(name: str, type_code: Any) -> Tuple:
    return (name, type_code, None, None, None, None, None)


_POINT_COLS = [_col("id", object), _col("score", float), _col("payload", dict)]
_VECTOR_COL = _col("vector", list)
_FACET_COLS = [_col("value", object), _col("count", int)]
_COUNT_COLS = [_col("count", int)]
_GROUP_COLS = [_col("group_id", object), _col("hits", list)]
_NAME_COLS = [_col("name", str)]


def _point_row(hit: Dict[str, Any], with_vector: bool) -> Tuple:
    try:
        score = float(hit.get("score", 0.0) or 0.0)
    except (TypeError, ValueError):
        score = 0.0
    payload = hit.get("payload")
    row = (hit.get("id"), score, dict(payload) if isinstance(payload, dict) else {})
    if with_vector:
        row = row + (hit.get("vector"),)
    return row


def _point_mapped(hits: List[Dict[str, Any]]) -> Tuple[List[Tuple], List[Tuple]]:
    with_vector = any(h.get("vector") is not None for h in hits)
    cols = list(_POINT_COLS) + ([_VECTOR_COL] if with_vector else [])
    return [_point_row(h, with_vector) for h in hits], cols


def map_result(report: Any, idx: int) -> Tuple[List[Tuple], Optional[List[Tuple]]]:
    """Map statement ``idx`` to ``(rows, description)`` (``None`` = no result set)."""
    results = report.results
    res = results[idx] if 0 <= idx < len(results) else {}
    op = res.get("operation", "")
    data = res.get("data", None)
    if op in POINT_OPS and isinstance(data, list):
        hits = [h for h in data if isinstance(h, dict) and "id" in h]
        return _point_mapped(hits)
    if op == "FACET" and isinstance(data, list):
        rows = [(e.get("value"), int(e.get("count", 0))) for e in data
                if isinstance(e, dict) and "value" in e and "count" in e]
        return rows, list(_FACET_COLS)
    if op == "QUERY_GROUPS" and isinstance(data, dict):
        rows = [(g.get("id", g.get("group_id")), g.get("hits", []))
                for g in report.groups(idx) if isinstance(g, dict)]
        return rows, list(_GROUP_COLS)
    if op == "COUNT" and isinstance(data, dict):
        return [(report.count(idx),)], list(_COUNT_COLS)
    if op == "SHOW_COLLECTIONS" and isinstance(data, dict):
        collections = data.get("collections", [])
        rows = [(name,) for name in collections if isinstance(name, str)]
        return rows, list(_NAME_COLS)
    if isinstance(data, list) and data and all(
        isinstance(h, dict) and "id" in h for h in data
    ):
        return _point_mapped(data)
    return [], None
