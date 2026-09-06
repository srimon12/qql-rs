from typing import Any, Dict, List, Optional, Tuple, Union

__version__: str

class QqlError(Exception):
    message: str
    code: Optional[str]
    kind: Optional[str]
    span: Optional[Tuple[int, int]]
    fields: Dict[str, str]
    request_id: Optional[str]
    def __init__(
        self,
        message: str,
        code: Optional[str] = None,
        kind: Optional[str] = None,
        span: Optional[Tuple[int, int]] = None,
        fields: Optional[Dict[str, str]] = None,
    ) -> None: ...

class QqlSyntaxError(QqlError, SyntaxError): ...
class QqlValidationError(QqlError, ValueError): ...
class QqlExecutionError(QqlError, RuntimeError): ...
class QqlTransportError(QqlError, RuntimeError): ...
class QqlBackendError(QqlError, RuntimeError): ...

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
    def groups(self, stmt: int = 0) -> List[Dict[str, Any]]: ...

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

class Client:
    # NOTE: Client has no constructor — instances come from
    # `local_executor()` / `http_executor()`.
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
    def compile(self, query: Union[str, Stmt]) -> Dict[str, Any]: ...
    def close(self) -> None: ...
    @property
    def is_closed(self) -> bool: ...
    def __enter__(self) -> "Client": ...
    def __exit__(self, *args: Any) -> None: ...

Query = Union[str, Stmt, List[Union[str, Stmt]]]

def parse(input: str) -> List[Stmt]: ...
def is_valid(input: str) -> bool: ...
def explain(query: Union[str, Stmt]) -> Dict[str, Any]: ...
def compile_query(query: str) -> Dict[str, Any]: ...
def tokenize(input: str) -> List[Dict[str, Any]]: ...
def inject_filter(query: Union[str, Stmt], field: str, op: str, value: Any) -> Stmt: ...
def bind(
    query: str, params: Optional[Union[Dict[str, Any], List[Any]]] = None
) -> str: ...
def parse_json(input: str) -> str: ...
def local_executor(
    data_dir: str,
    on_disk_payload: bool = True,
    *,
    model: Optional[str] = None,
    sparse_model: Optional[str] = None,
    multi_model: Optional[str] = None,
    image_model: Optional[str] = None,
    reranker_model: Optional[str] = None,
    cache_dir: Optional[str] = None,
    show_download_progress: bool = False,
) -> Client: ...
def http_executor(
    data_dir: str,
    url: str,
    embed_key: str,
    embed_model: str,
    embed_dim: int,
    on_disk_payload: bool = True,
) -> Client: ...
def list_embedding_models() -> List[Dict[str, Any]]: ...
def execute(
    query: Query,
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    data_dir: str = "./qdrant_data",
    on_disk_payload: bool = True,
    model: Optional[str] = None,
    sparse_model: Optional[str] = None,
    multi_model: Optional[str] = None,
    image_model: Optional[str] = None,
    reranker_model: Optional[str] = None,
    cache_dir: Optional[str] = None,
    show_download_progress: bool = False,
    on_error: str = "stop",
) -> Dict[str, Any]: ...
def execute_async(
    query: Query,
    *,
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    data_dir: str = "./qdrant_data",
    on_disk_payload: bool = True,
    model: Optional[str] = None,
    sparse_model: Optional[str] = None,
    multi_model: Optional[str] = None,
    image_model: Optional[str] = None,
    reranker_model: Optional[str] = None,
    cache_dir: Optional[str] = None,
    show_download_progress: bool = False,
    on_error: str = "stop",
) -> Dict[str, Any]: ...
