from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Union

from .pyqql import (
    Client as _Client,
    HttpEmbedder,
    Stmt,
    __version__,
    bind,
    compile_query,
    execute as _raw_execute,
    execute_async as _raw_execute_async,
    explain,
    inject_filter,
    is_valid,
    parse,
    parse_json,
    tokenize,
)


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


class Client(_Client):
    def execute(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> ExecutionReport:
        raw = super().execute(query, params=params, on_error=on_error)
        return ExecutionReport(raw)

    async def execute_async(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> ExecutionReport:
        raw = await super().execute_async(query, params=params, on_error=on_error)
        return ExecutionReport(raw)

    def execute_hits(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> List[ScoredPoint]:
        return self.execute(query, params=params, on_error=on_error).hits(0)

    async def execute_async_hits(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> List[ScoredPoint]:
        rep = await self.execute_async(query, params=params, on_error=on_error)
        return rep.hits(0)


def execute(
    query: Union[str, Stmt, List[Union[str, Stmt]]],
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> ExecutionReport:
    client = Client(
        url=url,
        api_key=api_key,
        use_grpc=use_grpc,
        embedder=embedder,
        route_affinity=route_affinity,
    )
    try:
        return client.execute(query, params=params, on_error=on_error)
    finally:
        client.close()


async def execute_async(
    query: Union[str, Stmt, List[Union[str, Stmt]]],
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> ExecutionReport:
    client = Client(
        url=url,
        api_key=api_key,
        use_grpc=use_grpc,
        embedder=embedder,
        route_affinity=route_affinity,
    )
    try:
        return await client.execute_async(query, params=params, on_error=on_error)
    finally:
        client.close()


def execute_hits(
    query: Union[str, Stmt, List[Union[str, Stmt]]],
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> List[ScoredPoint]:
    return execute(
        query,
        params=params,
        url=url,
        api_key=api_key,
        use_grpc=use_grpc,
        embedder=embedder,
        on_error=on_error,
        route_affinity=route_affinity,
    ).hits(0)


async def execute_async_hits(
    query: Union[str, Stmt, List[Union[str, Stmt]]],
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> List[ScoredPoint]:
    rep = await execute_async(
        query,
        params=params,
        url=url,
        api_key=api_key,
        use_grpc=use_grpc,
        embedder=embedder,
        on_error=on_error,
        route_affinity=route_affinity,
    )
    return rep.hits(0)


__all__ = [
    "Client",
    "HttpEmbedder",
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
    "inject_filter",
    "is_valid",
    "parse",
    "parse_json",
    "tokenize",
    "__version__",
]
