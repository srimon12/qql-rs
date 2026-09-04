from typing import Any, Dict, List, Optional, Union

__version__: str

class Stmt:
    # NOTE: Stmt has no constructor — instances come from `parse()`.
    @property
    def shard_key(self) -> Optional[str]: ...
    @shard_key.setter
    def shard_key(self, value: Optional[str]) -> None: ...
    def inject_filter(self, field: str, op: str, value: Any) -> None: ...
    def to_dict(self) -> Dict[str, Any]: ...
    def to_json(self) -> str: ...
    def compile_route(self) -> Dict[str, Any]: ...

class HttpEmbedder:
    def __init__(
        self,
        endpoint: str,
        model: str,
        dimension: int,
        api_key: Optional[str] = None,
    ) -> None: ...

class Client:
    def __init__(
        self,
        url: str = "http://localhost:6333",
        api_key: Optional[str] = None,
        use_grpc: bool = False,
        embedder: Optional[HttpEmbedder] = None,
        route_affinity: Optional[str] = None,
    ) -> None: ...
    @property
    def route_affinity(self) -> Optional[str]: ...
    def execute(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> Dict[str, Any]: ...
    def execute_async(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> Dict[str, Any]: ...
    def explain(self, query: Union[str, Stmt]) -> Dict[str, Any]: ...
    def compile(self, query: str) -> Dict[str, Any]: ...
    def close(self) -> None: ...
    def __enter__(self) -> "Client": ...
    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> Optional[bool]: ...

Query = Union[str, Stmt, List[Union[str, Stmt]]]

def parse(input: str) -> List[Stmt]: ...
def parse_json(input: str) -> str: ...
def is_valid(input: str) -> bool: ...
def explain(query: Union[str, Stmt]) -> Dict[str, Any]: ...
def compile_query(query: str) -> Dict[str, Any]: ...
def tokenize(input: str) -> List[Dict[str, Any]]: ...
def inject_filter(query: Union[str, Stmt], field: str, op: str, value: Any) -> Stmt: ...
def bind(
    query: str, params: Optional[Union[Dict[str, Any], List[Any]]] = None
) -> str: ...
def execute(
    query: Query,
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> Dict[str, Any]: ...
def execute_async(
    query: Query,
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> Dict[str, Any]: ...
