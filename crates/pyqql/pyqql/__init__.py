from typing import Any, Dict, List, Optional, Union

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
    execute as _raw_execute,
    execute_async as _raw_execute_async,
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
