"""Python package surface for the native :mod:`pyqql_edge` extension."""

from .pyqql_edge import (  # type: ignore[attr-defined]
    Client,
    Stmt,
    compile_query,
    execute,
    execute_async,
    explain,
    inject_filter,
    is_valid,
    local_executor,
    parse,
    parse_json,
    tokenize,
)

try:
    from .pyqql_edge import list_embedding_models
except ImportError:  # pragma: no cover - feature-disabled builds
    list_embedding_models = None

try:
    from .pyqql_edge import http_executor
except ImportError:  # pragma: no cover - feature-disabled builds
    http_executor = None

__all__ = [
    "Client",
    "Stmt",
    "compile_query",
    "execute",
    "execute_async",
    "explain",
    "http_executor",
    "inject_filter",
    "is_valid",
    "list_embedding_models",
    "local_executor",
    "parse",
    "parse_json",
    "tokenize",
]
