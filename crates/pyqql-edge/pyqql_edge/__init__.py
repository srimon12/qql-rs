"""Python package surface for the native :mod:`pyqql_edge` extension."""

from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Union

from .pyqql_edge import (  # type: ignore[attr-defined]
    Client,
    Stmt,
    __version__,
    bind,
    compile_query,
    explain,
    inject_filter,
    is_valid,
    parse,
    parse_json,
    tokenize,
)

try:
    from .pyqql_edge import (  # type: ignore[attr-defined]
        execute,
        execute_async,
        local_executor,
    )
except ImportError:  # pragma: no cover - feature-disabled builds
    execute = None  # type: ignore[assignment]
    execute_async = None  # type: ignore[assignment]
    local_executor = None  # type: ignore[assignment]

try:
    from .pyqql_edge import list_embedding_models  # type: ignore[attr-defined]
except ImportError:  # pragma: no cover - feature-disabled builds
    list_embedding_models = None

try:
    from .pyqql_edge import http_executor  # type: ignore[attr-defined]
except ImportError:  # pragma: no cover - feature-disabled builds
    http_executor = None


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


def execute_hits(*args: Any, **kwargs: Any) -> List[ScoredPoint]:
    if execute is None:
        raise NotImplementedError("execute is not available in this build")
    return execute(*args, **kwargs).hits(0)


async def execute_async_hits(*args: Any, **kwargs: Any) -> List[ScoredPoint]:
    if execute_async is None:
        raise NotImplementedError("execute_async is not available in this build")
    rep = await execute_async(*args, **kwargs)
    return rep.hits(0)


__all__ = [
    "Client",
    "Stmt",
    "ScoredPoint",
    "ExecutionReport",
    "bind",
    "compile_query",
    "execute",
    "execute_async",
    "execute_hits",
    "execute_async_hits",
    "explain",
    "http_executor",
    "inject_filter",
    "is_valid",
    "list_embedding_models",
    "local_executor",
    "parse",
    "parse_json",
    "tokenize",
    "__version__",
]
