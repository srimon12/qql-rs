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

class Client:
    # NOTE: Client has no constructor — instances come from
    # `local_executor()` / `http_executor()`.
    def execute(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        on_error: str = "stop",
    ) -> Dict[str, Any]: ...
    def execute_async(
        self,
        query: Union[str, Stmt, List[Union[str, Stmt]]],
        *,
        on_error: str = "stop",
    ) -> Dict[str, Any]: ...
    def explain(self, query: Union[str, Stmt]) -> Dict[str, Any]: ...
    def close(self) -> None: ...
    def __enter__(self) -> "Client": ...
    def __exit__(self, *args: Any) -> None: ...

Query = Union[str, Stmt, List[Union[str, Stmt]]]

def parse(input: str) -> List[Stmt]: ...
def is_valid(input: str) -> bool: ...
def explain(query: Union[str, Stmt]) -> Dict[str, Any]: ...
def compile_query(query: str) -> Dict[str, Any]: ...
def tokenize(input: str) -> List[Dict[str, Any]]: ...
def inject_filter(query: Union[str, Stmt], field: str, op: str, value: Any) -> Stmt: ...
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
