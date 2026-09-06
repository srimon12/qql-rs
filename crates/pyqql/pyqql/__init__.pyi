from typing import Any, Dict, List, Optional, Union

__version__: str

class ScoredPoint:
    id: Union[int, str]
    score: float
    payload: Optional[Dict[str, Any]]
    text: Optional[str]
    collection: Optional[str]
    def __getitem__(self, key: str) -> Any: ...
    def get(self, key: str, default: Any = None) -> Any: ...

class ExecutionReport(Dict[str, Any]):
    @property
    def ok(self) -> bool: ...
    @property
    def results(self) -> List[Dict[str, Any]]: ...
    @property
    def succeeded(self) -> int: ...
    @property
    def failed(self) -> int: ...
    def hits(self, stmt: int = 0) -> List[ScoredPoint]: ...
    def points(self, stmt: int = 0) -> List[ScoredPoint]: ...
    def facet(self, stmt: int = 0) -> List[Dict[str, Any]]: ...
    def count(self, stmt: int = 0) -> int: ...

class Stmt:
    # NOTE: Stmt has no constructor — instances come from `parse()`.
    @property
    def shard_key(self) -> Optional[str]: ...
    @shard_key.setter
    def shard_key(self, value: Optional[str]) -> None: ...
    def inject_filter(self, field: str, op: str, value: Any) -> None: ...
    def to_dict(self) -> Dict[str, Any]: ...
    def to_json(self) -> str: ...
    def bind(
        self, params: Optional[Union[Dict[str, Any], List[Any]]] = None
    ) -> "Stmt": ...
    def compile_route(
        self, params: Optional[Union[Dict[str, Any], List[Any]]] = None
    ) -> Dict[str, Any]: ...
    def explain(self) -> Dict[str, Any]: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...

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
    ) -> ExecutionReport: ...
    async def execute_async(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> ExecutionReport: ...
    def execute_hits(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> List[ScoredPoint]: ...
    async def execute_async_hits(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> List[ScoredPoint]: ...
    def explain(self, query: Union[str, Stmt]) -> Dict[str, Any]: ...
    def compile(self, query: str, params: Optional[Union[Dict[str, Any], List[Any]]] = None) -> Dict[str, Any]: ...
    def close(self) -> None: ...
    def __enter__(self) -> "Client": ...
    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> Optional[bool]: ...

Query = Union[str, Stmt, List[Union[str, Stmt]]]

def parse(input: str) -> List[Stmt]: ...
def parse_json(input: str) -> str: ...
def is_valid(input: str) -> bool: ...
def explain(query: Union[str, Stmt]) -> Dict[str, Any]: ...
def compile_query(query: str, params: Optional[Union[Dict[str, Any], List[Any]]] = None) -> Dict[str, Any]: ...
def tokenize(input: str) -> List[Dict[str, Any]]: ...
def inject_filter(query: Union[str, Stmt], field: str, op: str, value: Any) -> Stmt: ...
def bind(
    query: Union[str, Stmt],
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    *,
    truncate_vectors: bool = False,
) -> Union[str, Stmt]: ...
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
) -> ExecutionReport: ...
async def execute_async(
    query: Query,
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> ExecutionReport: ...
def execute_hits(
    query: Query,
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> List[ScoredPoint]: ...
async def execute_async_hits(
    query: Query,
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    url: str = "http://localhost:6333",
    api_key: Optional[str] = None,
    use_grpc: bool = False,
    embedder: Optional[HttpEmbedder] = None,
    on_error: str = "stop",
    route_affinity: Optional[str] = None,
) -> List[ScoredPoint]: ...
