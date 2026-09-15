"""Python package surface for the native :mod:`pyqql_edge` extension.

Client ``execute`` / ``upsert_many`` params convert like ``Stmt.bind``: flat
``list[float]`` values with 32+ elements bind as f32 vectors (the same
precision as numpy / ``array.array`` buffers); nested lists, int/bool lists,
and shorter float lists keep f64 precision.
"""

from typing import Any, AsyncIterator, Dict, Iterator, List, Optional, Union

from ._errors import (
    QqlError,
    QqlSyntaxError,
    QqlValidationError,
    QqlExecutionError,
    QqlTransportError,
    QqlBackendError,
)
from .pyqql_edge import (  # type: ignore[attr-defined]
    Client,
    ExecutionReport,
    ScoredPoint,
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


def execute_hits(*args: Any, **kwargs: Any) -> List[ScoredPoint]:
    if execute is None:
        raise NotImplementedError("execute is not available in this build")
    return execute(*args, **kwargs).hits(0)


async def execute_async_hits(*args: Any, **kwargs: Any) -> List[ScoredPoint]:
    if execute_async is None:
        raise NotImplementedError("execute_async is not available in this build")
    rep = await execute_async(*args, **kwargs)
    return rep.hits(0)


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
    return point.without_payload()


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


def scroll_cursor(
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
    """Lazily page through `collection` with `SCROLL`, yielding one `ScoredPoint` per point.

    Module-level form (mirrors `nqql-edge`'s `scrollCursor(client, …)`):
    works with any client exposing `execute(sql, { params })` — edge
    `Client` instances come from `local_executor()` / `http_executor()`
    factories, so the helpers take the client instead of living on it.
    Pages fetch one at a time, so at most one page is buffered no matter
    how large the collection is. Iteration stops on the first empty page.
    """
    yield from _scroll_cursor_impl(
        client, collection, batch_size=batch_size, where=where, params=params,
        with_payload=with_payload, with_vector=with_vector, shard_key=shard_key,
    )


async def scroll_cursor_async(
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
    """Async variant of `scroll_cursor` using `execute_async` for each page."""
    async for point in _scroll_cursor_async_impl(
        client, collection, batch_size=batch_size, where=where, params=params,
        with_payload=with_payload, with_vector=with_vector, shard_key=shard_key,
    ):
        yield point


__all__ = [
    "Client",
    "Stmt",
    "ScoredPoint",
    "ExecutionReport",
    "QqlError",
    "QqlSyntaxError",
    "QqlValidationError",
    "QqlExecutionError",
    "QqlTransportError",
    "QqlBackendError",
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
    "scroll_cursor",
    "scroll_cursor_async",
    "tokenize",
    "__version__",
]
