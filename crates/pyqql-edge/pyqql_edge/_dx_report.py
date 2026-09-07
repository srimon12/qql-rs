"""Typed execution-result classes shared byte-identically by the pyqql
and pyqql-edge Python wrappers.

A CI check diffs the two copies of this file, so edit both or neither
(they must stay in lockstep with the JS ``dx-common.js`` classes).
"""

from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Union

@dataclass
class ScoredPoint:
    """A scored hit returned from a search or retrieval query."""

    id: Union[int, str]
    score: float = 0.0
    payload: Optional[Dict[str, Any]] = None
    text: Optional[str] = None
    collection: Optional[str] = None
    vector: Optional[Any] = None
    shard_key: Optional[Union[str, int]] = None

    def __getitem__(self, key: str) -> Any:
        if self.payload and key in self.payload:
            return self.payload[key]
        raise KeyError(key)

    def get(self, key: str, default: Any = None) -> Any:
        if self.payload and key in self.payload:
            return self.payload[key]
        return default


class ExecutionReport(dict):
    """Execution report returned by Client.execute().

    Subclasses `dict` for 100% backward compatibility (`report["ok"]`, `report["results"]`),
    while providing typed helper accessors (`.hits()`, `.points()`, `.facet()`, `.count()`).
    """

    @property
    def ok(self) -> bool:
        return self.get("ok", False)

    @property
    def results(self) -> List[Dict[str, Any]]:
        return self.get("results", [])

    @property
    def succeeded(self) -> int:
        return self.get("succeeded", 0)

    @property
    def failed(self) -> int:
        return self.get("failed", 0)

    def _result_at(self, stmt: int) -> Optional[Dict[str, Any]]:
        res = self.results
        if not res or stmt >= len(res) or stmt < -len(res):
            return None
        return res[stmt]

    def hits(self, stmt: int = 0) -> List[ScoredPoint]:
        """Return typed ScoredPoint objects for statement `stmt` (default first statement)."""
        r = self._result_at(stmt)
        if not r:
            return []
        data = r.get("data")
        if not isinstance(data, list):
            return []
        out = []
        for h in data:
            # Only map entries shaped like scored points; facet entries
            # ({value, count}) are not ScoredPoints.
            if isinstance(h, dict) and "id" in h and "score" in h:
                score_val = h.get("score")
                score = float(score_val) if score_val is not None else 0.0
                out.append(
                    ScoredPoint(
                        id=h.get("id"),
                        score=score,
                        payload=h.get("payload"),
                        text=h.get("text"),
                        collection=h.get("collection"),
                        vector=h.get("vector"),
                        shard_key=h.get("shard_key"),
                    )
                )
        return out

    def points(self, stmt: int = 0) -> List[ScoredPoint]:
        """Alias for hits(stmt)."""
        return self.hits(stmt)

    def facet(self, stmt: int = 0) -> List[Dict[str, Any]]:
        """Return facet hits list for statement `stmt`."""
        r = self._result_at(stmt)
        if not r:
            return []
        data = r.get("data")
        if isinstance(data, list):
            return data
        if isinstance(data, dict):
            return data.get("result", {}).get("hits", data.get("hits", []))
        return []

    def count(self, stmt: int = 0) -> int:
        """Return count integer for statement `stmt`."""
        r = self._result_at(stmt)
        if not r:
            return 0
        data = r.get("data")
        if isinstance(data, dict):
            c = data.get("result", {}).get("count", data.get("count"))
            if c is not None:
                return int(c)
        msg = r.get("message", "")
        if msg.startswith("Count: "):
            try:
                return int(msg.split(": ")[1])
            except (IndexError, ValueError):
                pass
        return 0

    def groups(self, stmt: int = 0) -> List[Dict[str, Any]]:
        """Return ``GROUP BY`` groups for statement `stmt` (default first).

        Each group is the raw backend group object (``{"id": <group key>,
        "hits": [<point records>]}``) — the same shape the server returns,
        normalized across the ``{"result": {"groups": [...]}}`` and bare
        ``{"groups": [...]}`` envelopes.
        """
        r = self._result_at(stmt)
        if not r:
            return []
        data = r.get("data")
        if isinstance(data, dict):
            result = data.get("result")
            nested = result.get("groups") if isinstance(result, dict) else None
            groups = nested if nested is not None else data.get("groups")
            if isinstance(groups, list):
                return groups
        return []

