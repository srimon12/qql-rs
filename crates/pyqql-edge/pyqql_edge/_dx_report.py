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
    score: float
    payload: Optional[Dict[str, Any]] = None
    text: Optional[str] = None
    collection: Optional[str] = None

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

    def hits(self, stmt: int = 0) -> List[ScoredPoint]:
        """Return typed ScoredPoint objects for statement `stmt` (default first statement)."""
        res = self.results
        if not res or stmt >= len(res):
            return []
        data = res[stmt].get("data")
        if not isinstance(data, list):
            return []
        return [
            ScoredPoint(
                id=h.get("id"),
                score=float(h.get("score", 0.0)),
                payload=h.get("payload"),
                text=h.get("text"),
                collection=h.get("collection"),
            )
            for h in data
            if isinstance(h, dict)
        ]

    def points(self, stmt: int = 0) -> List[ScoredPoint]:
        """Alias for hits(stmt)."""
        return self.hits(stmt)

    def facet(self, stmt: int = 0) -> List[Dict[str, Any]]:
        """Return facet hits list for statement `stmt`."""
        res = self.results
        if not res or stmt >= len(res):
            return []
        data = res[stmt].get("data")
        if isinstance(data, list):
            return data
        if isinstance(data, dict):
            return data.get("result", {}).get("hits", data.get("hits", []))
        return []

    def count(self, stmt: int = 0) -> int:
        """Return count integer for statement `stmt`."""
        res = self.results
        if not res or stmt >= len(res):
            return 0
        r = res[stmt]
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
        res = self.results
        if not res or stmt >= len(res):
            return []
        data = res[stmt].get("data")
        if isinstance(data, dict):
            result = data.get("result")
            nested = result.get("groups") if isinstance(result, dict) else None
            groups = nested if nested is not None else data.get("groups")
            if isinstance(groups, list):
                return groups
        return []
