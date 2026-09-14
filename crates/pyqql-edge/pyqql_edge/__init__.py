"""Python package surface for the native :mod:`pyqql_edge` extension.

Client ``execute`` / ``upsert_many`` params convert like ``Stmt.bind``: flat
``list[float]`` values with 32+ elements bind as f32 vectors (the same
precision as numpy / ``array.array`` buffers); nested lists, int/bool lists,
and shorter float lists keep f64 precision.
"""

from typing import Any, List

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
    "tokenize",
    "__version__",
]
