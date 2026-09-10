from typing import Any, AsyncIterator, Dict, Iterator, List, Optional, Union

from ._dbapi import (
    Connection,
    Cursor,
    DatabaseError,
    DataError,
    Error,
    IntegrityError,
    InterfaceError,
    InternalError,
    NotSupportedError,
    OperationalError,
    ProgrammingError,
    Warning,
    apilevel,
    connect,
    paramstyle,
    threadsafety,
)
from ._dx_report import ExecutionReport, ScoredPoint
from ._errors import (
    QqlError,
    QqlSyntaxError,
    QqlValidationError,
    QqlExecutionError,
    QqlTransportError,
    QqlBackendError,
)
from .pyqql import (
    Client as _Client,
    HttpEmbedder,
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

    def upsert_many(
        self,
        collection: str,
        rows: List[Dict[str, Any]],
        *,
        batch_size: int = 100,
        on_error: str = "stop",
    ) -> ExecutionReport:
        """Bulk ingest `rows` (point dicts) in `batch_size` chunks.

        One `:rows` template is prepared once; each chunk splices through
        the point-splice path with no re-parse and no per-batch schema
        fetch. Prefer this over hand-rolled batch loops.
        """
        raw = super().upsert_many(
            collection, rows, batch_size=batch_size, on_error=on_error
        )
        return ExecutionReport(raw)

    def scroll_cursor(
        self,
        collection: str,
        *,
        batch_size: int = 100,
        where: str = "",
        params: Optional[Dict[str, Any]] = None,
        with_payload: bool = True,
        with_vector: bool = False,
        shard_key: Optional[Union[str, int]] = None,
    ) -> Iterator[ScoredPoint]:
        """Lazily page through `collection` with `SCROLL`, yielding one `ScoredPoint` per point.

        Pages fetch one at a time through `execute`, so at most one page is
        buffered no matter how large the collection is. Iteration stops on the
        first empty page. See the API surface bulk and scroll docs for the
        shared contract.
        """
        yield from _scroll_cursor_impl(
            self, collection, batch_size=batch_size, where=where, params=params,
            with_payload=with_payload, with_vector=with_vector, shard_key=shard_key,
        )

    async def scroll_cursor_async(
        self,
        collection: str,
        *,
        batch_size: int = 100,
        where: str = "",
        params: Optional[Dict[str, Any]] = None,
        with_payload: bool = True,
        with_vector: bool = False,
        shard_key: Optional[Union[str, int]] = None,
    ) -> AsyncIterator[ScoredPoint]:
        """Async variant of `scroll_cursor` using `execute_async` for each page."""
        async for point in _scroll_cursor_async_impl(
            self, collection, batch_size=batch_size, where=where, params=params,
            with_payload=with_payload, with_vector=with_vector, shard_key=shard_key,
        ):
            yield point


def _format_collection(name: str) -> str:
    import re

    if re.match(r"^[a-zA-Z_][a-zA-Z0-9_]*$", name):
        return name
    if (name.startswith('"') and name.endswith('"')) or (
        name.startswith("'") and name.endswith("'")
    ):
        return name
    return '"' + name.replace('"', '""') + '"'


def _validate_scroll_args(
    collection: Any,
    batch_size: Any,
    where: Any,
    params: Any,
    with_payload: Any,
    with_vector: Any,
    shard_key: Any,
) -> tuple[str, int, str, Optional[Dict[str, Any]], bool, bool, Optional[Union[str, int]]]:
    if not isinstance(collection, str) or not collection:
        raise TypeError("scroll_cursor requires a non-empty collection name string")
    if isinstance(batch_size, bool) or not isinstance(batch_size, int) or batch_size < 1:
        raise ValueError("batch_size must be an integer greater than or equal to 1")
    if not isinstance(where, str):
        raise TypeError("where must be a QQL filter string")
    if not isinstance(with_payload, bool):
        raise TypeError("with_payload must be a boolean")
    if not isinstance(with_vector, bool):
        raise TypeError("with_vector must be a boolean")
    if shard_key is not None and not isinstance(shard_key, (str, int)):
        raise TypeError("shard_key must be a string, an integer, or None")
    if isinstance(shard_key, bool):
        raise TypeError("shard_key must be a string, an integer, or None (bool is not a shard key)")
    if isinstance(shard_key, int) and shard_key < 0:
        raise ValueError("shard_key integer must fit in u64 (non-negative)")
    if params is not None:
        if not isinstance(params, dict):
            raise TypeError("params must be a dict with named parameters")
        if "cursor" in params:
            raise TypeError('params cannot contain reserved parameter "cursor"')
    return (collection, batch_size, where.strip(), params, with_payload, with_vector, shard_key)


def _build_scroll_statement(
    collection: str,
    batch_size: int,
    where: str,
    cursor: Any,
    base_params: Optional[Dict[str, Any]],
    with_vector: bool,
    shard_key: Optional[Union[str, int]],
) -> tuple[str, Optional[Dict[str, Any]]]:
    sql = "SCROLL FROM " + _format_collection(collection)
    if where:
        sql += " WHERE " + where
    params: Optional[Dict[str, Any]] = dict(base_params) if base_params else None
    if cursor is not None:
        sql += " AFTER :cursor"
        if params is None:
            params = {"cursor": cursor}
        else:
            params["cursor"] = cursor
    if shard_key is not None:
        if isinstance(shard_key, int):
            sql += " SHARD " + str(shard_key)
        else:
            sql += " SHARD '" + str(shard_key).replace("'", "''") + "'"
    if with_vector:
        sql += " WITH VECTOR"
    sql += " LIMIT " + str(batch_size)
    return (sql, params)


def _strip_payload(point: ScoredPoint) -> ScoredPoint:
    return ScoredPoint(
        id=point.id,
        score=point.score,
        payload=None,
        text=point.text,
        collection=point.collection,
        vector=getattr(point, "vector", None),
        shard_key=getattr(point, "shard_key", None),
    )


def _scroll_cursor_impl(
    client: Any,
    collection: str,
    *,
    batch_size: int = 100,
    where: str = "",
    params: Optional[Dict[str, Any]] = None,
    with_payload: bool = True,
    with_vector: bool = False,
    shard_key: Optional[Union[str, int]] = None,
) -> Iterator[ScoredPoint]:
    # Shared validation and statement building for the sync generator.
    # The async variant uses _scroll_cursor_async_impl instead.
    if not hasattr(client, "execute") or not callable(getattr(client, "execute")):
        raise TypeError("scroll_cursor requires a client with an execute() method")
    (_coll, _batch, _where, _params, _with_payload, _with_vector, _shard) = _validate_scroll_args(
        collection, batch_size, where, params, with_payload, with_vector, shard_key
    )
    cursor: Any = None
    first = True
    while True:
        (sql, page_params) = _build_scroll_statement(
            _coll, _batch, _where, None if first else cursor, _params, _with_vector, _shard
        )
        report = client.execute(sql, params=page_params) if page_params is not None else client.execute(sql)
        if not isinstance(report, ExecutionReport):
            report = ExecutionReport(report)
        hits = report.hits()
        if not hits:
            return
        next_cursor = hits[-1].id
        if not first and next_cursor == cursor:
            return
        for hit in hits:
            yield hit if _with_payload else _strip_payload(hit)
        cursor = next_cursor
        first = False


async def _scroll_cursor_async_impl(
    client: Any,
    collection: str,
    *,
    batch_size: int = 100,
    where: str = "",
    params: Optional[Dict[str, Any]] = None,
    with_payload: bool = True,
    with_vector: bool = False,
    shard_key: Optional[Union[str, int]] = None,
) -> AsyncIterator[ScoredPoint]:
    if not hasattr(client, "execute_async") or not callable(getattr(client, "execute_async")):
        raise TypeError("scroll_cursor_async requires a client with an execute_async() method")
    (_coll, _batch, _where, _params, _with_payload, _with_vector, _shard) = _validate_scroll_args(
        collection, batch_size, where, params, with_payload, with_vector, shard_key
    )
    cursor: Any = None
    first = True
    while True:
        (sql, page_params) = _build_scroll_statement(
            _coll, _batch, _where, None if first else cursor, _params, _with_vector, _shard
        )
        if page_params is not None:
            report = await client.execute_async(sql, params=page_params)
        else:
            report = await client.execute_async(sql)
        if not isinstance(report, ExecutionReport):
            report = ExecutionReport(report)
        hits = report.hits()
        if not hits:
            return
        next_cursor = hits[-1].id
        if not first and next_cursor == cursor:
            return
        for hit in hits:
            yield hit if _with_payload else _strip_payload(hit)
        cursor = next_cursor
        first = False


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
    "QqlError",
    "QqlSyntaxError",
    "QqlValidationError",
    "QqlExecutionError",
    "QqlTransportError",
    "QqlBackendError",
    "connect",
    "Connection",
    "Cursor",
    "Error",
    "Warning",
    "InterfaceError",
    "DatabaseError",
    "DataError",
    "OperationalError",
    "IntegrityError",
    "InternalError",
    "ProgrammingError",
    "NotSupportedError",
    "apilevel",
    "threadsafety",
    "paramstyle",
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
