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

# DB-API names (defined in the shared `_errors.py`; not re-exported at the
# `pyqql_edge` package root, which stays parser/client-only).
Error = QqlError

class Warning(Exception): ...
class DatabaseError(Error): ...
class InterfaceError(Error): ...
class DataError(DatabaseError): ...
class OperationalError(DatabaseError): ...
class IntegrityError(DatabaseError): ...
class InternalError(DatabaseError): ...
class ProgrammingError(DatabaseError): ...
class NotSupportedError(DatabaseError): ...

class QqlSyntaxError(ProgrammingError, SyntaxError): ...
class QqlValidationError(ProgrammingError, DataError, ValueError): ...
class QqlExecutionError(OperationalError, RuntimeError): ...
class QqlTransportError(OperationalError, RuntimeError): ...
class QqlBackendError(OperationalError, RuntimeError): ...

class ScoredPoint:
    """Native typed hit returned by ExecutionReport.hits()."""

    id: Union[int, str]
    score: float
    payload: Optional[Dict[str, Any]]
    text: Optional[str]
    collection: Optional[str]
    vector: Optional[Any]
    def __getitem__(self, key: str) -> Any: ...
    def get(self, key: str, default: Any = None) -> Any: ...
    def without_payload(self) -> "ScoredPoint": ...
    def __repr__(self) -> str: ...

class ExecutionReport:
    """Native typed report returned by Client.execute()."""

    @classmethod
    def from_results(cls, results: List[Dict[str, Any]]) -> "ExecutionReport": ...
    @property
    def ok(self) -> bool: ...
    @property
    def results(self) -> List[Dict[str, Any]]: ...
    @property
    def succeeded(self) -> int: ...
    @property
    def failed(self) -> int: ...
    @property
    def telemetry(self) -> Optional[Dict[str, Any]]: ...
    def hits(self, stmt: int = 0) -> List[ScoredPoint]: ...
    def points(self, stmt: int = 0) -> List[ScoredPoint]: ...
    def ids(self, stmt: int = 0) -> List[Any]: ...
    def facet(self, stmt: int = 0) -> List[Dict[str, Any]]: ...
    def count(self, stmt: int = 0) -> int: ...
    def groups(self, stmt: int = 0) -> List[Dict[str, Any]]: ...
    def collections(self, stmt: int = 0) -> List[str]: ...
    def collection(self, stmt: int = 0) -> Optional[Dict[str, Any]]: ...
    def shard_keys(self, stmt: int = 0) -> List[Any]: ...
    def quotas(self, stmt: int = 0) -> Optional[Dict[str, Any]]: ...
    def __getitem__(self, key: str) -> Any: ...
    def get(self, key: str, default: Any = None) -> Any: ...
    def __repr__(self) -> str: ...

class Stmt:
    # NOTE: Stmt has no constructor — instances come from `parse()`.
    @property
    def shard_key(self) -> Optional[Union[str, int]]:
        """Keyword keys read as `str`, numeric keys as `int` (`None` when unset)."""
        ...
    @shard_key.setter
    def shard_key(self, value: Optional[Union[str, int]]) -> None: ...
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

class Client:
    # NOTE: Client has no constructor — instances come from
    # `local_executor()` / `http_executor()`.
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
    def explain_analyze(
        self,
        query: Union[str, Stmt],
        *,
        params: Optional[Union[Dict[str, Any], List[Any]]] = None,
        on_error: str = "stop",
    ) -> Dict[str, Any]: ...
    def compile(
        self, query: str, params: Optional[Union[Dict[str, Any], List[Any]]] = None
    ) -> Dict[str, Any]: ...
    def upsert_many(
        self,
        collection: str,
        rows: List[Dict[str, Any]],
        *,
        batch_size: int = 100,
        on_error: str = "stop",
    ) -> ExecutionReport: ...
    def optimize(self, collection: str) -> bool:
        """Run an in-process storage optimization pass on `collection`."""
        ...
    def close(self) -> None: ...
    @property
    def is_closed(self) -> bool: ...
    def __enter__(self) -> "Client": ...
    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> Optional[bool]: ...

Query = Union[str, Stmt, List[Union[str, Stmt]]]

def parse(input: str) -> List[Stmt]: ...
def parse_json(input: str) -> str: ...
def is_valid(input: str) -> bool: ...
def explain(query: Union[str, Stmt]) -> Dict[str, Any]: ...
def compile_query(
    query: str, params: Optional[Union[Dict[str, Any], List[Any]]] = None
) -> Dict[str, Any]: ...
def tokenize(input: str) -> List[Dict[str, Any]]: ...
def inject_filter(query: Union[str, Stmt], field: str, op: str, value: Any) -> Stmt: ...
def bind(
    query: Union[str, Stmt],
    params: Optional[Union[Dict[str, Any], List[Any]]] = None,
    *,
    truncate_vectors: bool = False,
) -> Union[str, Stmt]: ...
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
    wal_segment_mb: Optional[int] = None,
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
) -> ExecutionReport: ...
async def execute_async(
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
) -> ExecutionReport: ...
def execute_hits(
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
) -> List[ScoredPoint]: ...
async def execute_async_hits(
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
) -> List[ScoredPoint]: ...
