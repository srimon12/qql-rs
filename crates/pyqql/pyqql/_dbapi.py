"""PEP 249 (DB-API 2.0) pragmatic-subset wrapper over :class:`pyqql.Client`.

Qdrant is not relational — no tables, joins, transactions, or stored
procedures — so this is a deliberately small surface: :func:`connect`,
:class:`Connection` (``cursor`` / ``close`` / ``commit``-as-no-op /
``rollback``-raises), and :class:`Cursor` (``execute`` / ``executemany`` /
``fetchone`` / ``fetchmany`` / ``fetchall`` / iteration / ``description`` /
``rowcount``). Excluded per spec: ``callproc``, ``setinputsizes``,
``setoutputsize`` — all raise :class:`NotSupportedError`.

QQL idioms honored here: payloads arrive by default (no redundant
``WITH PAYLOAD`` clauses are emitted), vector queries use compact literals
(``QUERY [0.1, ...] FROM ...``), and nothing imports ``qdrant_client``.

Errors are the single hierarchy in :mod:`pyqql._errors`: the native
``Qql*`` classes already subclass the DB-API names below, so failures
propagate untouched — no translation, no ``raise X from Y`` re-wraps.
Result mapping in :mod:`pyqql._dbapi_rows`.
"""

from typing import Any, Dict, Iterable, List, NoReturn, Optional, Tuple

from ._errors import (
    DatabaseError,
    DataError,
    Error,
    IntegrityError,
    InterfaceError,
    InternalError,
    NotSupportedError,
    OperationalError,
    ProgrammingError,
    QqlExecutionError,
    Warning,
)
from ._dbapi_rows import map_result
from ._dx_report import ExecutionReport
from .pyqql import parse as _parse_statements

apilevel = "2.0"
threadsafety = 1
paramstyle = "named"

__all__ = [
    "apilevel",
    "threadsafety",
    "paramstyle",
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
]


def _jsonable(value: Any) -> Any:
    """Deep-convert tuples to lists so positional params bind natively."""
    if isinstance(value, tuple):
        return [_jsonable(v) for v in value]
    if isinstance(value, list):
        return [_jsonable(v) for v in value]
    if isinstance(value, dict):
        return {k: _jsonable(v) for k, v in value.items()}
    return value


def _reraise_closed_as_interface(exc: QqlExecutionError) -> NoReturn:
    """Refine ``QQL-CLIENT-CLOSED`` to ``InterfaceError``; re-raise the rest.

    The Rust kind system has no interface kind, so the native client reports
    a closed client as ``QqlExecutionError``. PEP 249 assigns handle misuse
    to ``InterfaceError``, hence this one narrow code check — the only
    native-error remap left. Everything else propagates untouched: it is
    already a DB-API error by inheritance.
    """
    if getattr(exc, "code", None) == "QQL-CLIENT-CLOSED":
        raise InterfaceError(
            str(exc),
            code=exc.code,
            kind=exc.kind,
            span=exc.span,
            fields=dict(exc.fields),
        ) from exc
    raise exc


def connect(*args: Any, **kwargs: Any) -> "Connection":
    """Open a DB-API connection wrapping ``Client(*args, **kwargs)``."""
    return Connection(*args, **kwargs)


class Connection:
    """DB-API connection: owns a :class:`pyqql.Client` (auto-commit)."""

    def __init__(self, client: Any = None, *args: Any, **kwargs: Any) -> None:
        if client is not None and hasattr(client, "execute") and callable(getattr(client, "execute")):
            if args or kwargs:
                raise TypeError("pass either a client or Client() arguments, not both")
            self.client = client
        else:
            from pyqql import Client  # deferred: breaks the package-init cycle

            all_args = (client, *args) if client is not None else args
            self.client = Client(*all_args, **kwargs)
        self._closed = False

    def cursor(self) -> "Cursor":
        if self._closed:
            raise InterfaceError("connection is closed")
        return Cursor(self)

    def commit(self) -> None:
        """No-op: every statement auto-commits (use ``WAIT true`` when durability matters)."""

    def rollback(self) -> None:
        raise NotSupportedError(
            "rollback is not supported: statements auto-commit, "
            "there is nothing to roll back"
        )

    def close(self) -> None:
        if self._closed:
            return
        try:
            close = getattr(self.client, "close", None)
            if callable(close):
                close()
        finally:
            self._closed = True

    def __enter__(self) -> "Connection":
        return self

    def __exit__(self, *exc_info: Any) -> None:
        self.close()


class Cursor:
    """DB-API cursor: executes QQL through the owning connection's client."""

    arraysize: int = 1

    def __init__(self, connection: Connection) -> None:
        self.connection = connection
        self._closed = False
        self._rows: List[Tuple] = []
        self._pos = 0
        self.description: Optional[List[Tuple]] = None
        self.rowcount: int = -1
        self._result_sets: List[Tuple[List[Tuple], Optional[List[Tuple]], int]] = []
        self._set_idx = 0

    # -- internals ---------------------------------------------------

    def _ensure_open(self) -> None:
        if self._closed:
            raise InterfaceError("cursor is closed")
        if self.connection._closed:
            raise InterfaceError("connection is closed")

    def _reset(self) -> None:
        self._rows = []
        self._pos = 0
        self.description = None
        self.rowcount = -1
        self._result_sets = []
        self._set_idx = 0

    def _ingest(self, raw: Any, is_executemany: bool = False) -> None:
        rep = raw if isinstance(raw, ExecutionReport) else ExecutionReport(raw)
        results = rep.get("results", [])
        for idx, res in enumerate(results):
            if not res.get("ok", False):
                raise OperationalError(
                    "statement {} failed [{}]: {}".format(
                        idx, res.get("operation", "?"), res.get("message", "")
                    )
                )

        if is_executemany:
            rows: List[Tuple] = []
            description: Optional[List[Tuple]] = None
            upsert_total = 0
            saw_upsert = False
            for idx, res in enumerate(results):
                if res.get("operation") == "UPSERT":
                    saw_upsert = True
                    data = res.get("data") or {}
                    try:
                        upsert_total += int(data.get("count", 0))
                    except (TypeError, ValueError):
                        pass
                    continue
                part_rows, part_desc = map_result(rep, idx)
                rows.extend(part_rows)
                if description is None and part_desc is not None:
                    description = part_desc
            self._rows = rows
            self._pos = 0
            self.description = description
            if description is not None:
                self.rowcount = len(rows)
            elif saw_upsert:
                self.rowcount = upsert_total
            else:
                self.rowcount = -1
            self._result_sets = [(self._rows, self.description, self.rowcount)]
            self._set_idx = 0
            return

        self._result_sets = []
        for idx, res in enumerate(results):
            op = res.get("operation")
            if op == "UPSERT":
                data = res.get("data") or {}
                count = 0
                try:
                    count = int(data.get("count", 0))
                except (TypeError, ValueError):
                    pass
                self._result_sets.append(([], None, count))
                continue
            part_rows, part_desc = map_result(rep, idx)
            rc = len(part_rows) if part_desc is not None else -1
            self._result_sets.append((part_rows, part_desc, rc))

        if not self._result_sets:
            self._result_sets = [([], None, -1)]

        self._set_idx = 0
        self._rows, self.description, self.rowcount = self._result_sets[0]
        self._pos = 0

    def _run(self, sql: Any, params: Any) -> None:
        self._ensure_open()
        self._reset()
        try:
            raw = self.connection.client.execute(
                sql, params=_jsonable(params) if params is not None else None
            )
        except QqlExecutionError as exc:
            _reraise_closed_as_interface(exc)
        self._ingest(raw, is_executemany=False)

    # -- execution ---------------------------------------------------

    def execute(self, sql: Any, params: Any = None) -> "Cursor":
        """Execute QQL (single statement, script, or list) and cache result rows."""
        self._run(sql, params)
        return self

    def executemany(self, sql: Any, seq_of_params: Iterable[Any]) -> "Cursor":
        """Bulk path: ``UPSERT ... VALUES :rows`` delegates to
        ``client.upsert_many``; anything else runs as one statement-scoped
        batch (``[sql] * n`` with per-statement params)."""
        self._ensure_open()
        self._reset()
        if isinstance(seq_of_params, (str, bytes, dict)):
            raise ProgrammingError(
                "executemany requires a sequence of parameter sets "
                f"(got {type(seq_of_params).__name__})"
            )
        try:
            items = [_jsonable(p) if p is not None else {} for p in seq_of_params]
        except TypeError as exc:
            raise ProgrammingError(
                "executemany requires a sequence of parameter sets "
                f"(got {type(seq_of_params).__name__})"
            ) from exc
        if not items:
            self._rows = []
            self.description = None
            self.rowcount = 0
            return self
        stmt = sql if not isinstance(sql, str) else self._parse_one(sql)
        upsert = self._upsert_target(stmt)
        try:
            if upsert is not None:
                self._executemany_upsert(upsert, items)
            else:
                raw = self.connection.client.execute([sql] * len(items), params=items)
                self._ingest(raw, is_executemany=True)
        except QqlExecutionError as exc:
            _reraise_closed_as_interface(exc)
        return self

    @staticmethod
    def _parse_one(sql: str) -> Any:
        # Native parse failures are already ProgrammingError by inheritance.
        stmts = _parse_statements(sql)
        if len(stmts) != 1:
            raise ProgrammingError(
                f"executemany requires a single statement (got {len(stmts)})"
            )
        return stmts[0]

    @staticmethod
    def _upsert_target(stmt: Any) -> Optional[str]:
        """Collection name when ``stmt`` is an ``UPSERT ... VALUES :rows`` template."""
        try:
            shape = stmt.to_dict() if hasattr(stmt, "to_dict") else {}
        except Exception:
            return None
        upsert = shape.get("Upsert") if isinstance(shape, dict) else None
        if not isinstance(upsert, dict) or not upsert.get("collection"):
            return None
        if ":rows" not in str(stmt):
            return None
        collection = upsert["collection"]
        return collection if isinstance(collection, str) else None

    def _executemany_upsert(self, collection: str, items: List[Any]) -> None:
        rows: List[Dict[str, Any]] = []
        if all(isinstance(p, dict) and "rows" not in p for p in items):
            rows = list(items)
        elif all(isinstance(p, dict) and "rows" in p for p in items):
            for p in items:
                v = p["rows"]
                if isinstance(v, dict):
                    rows.append(v)
                elif isinstance(v, list):
                    rows.extend(v)
                else:
                    raise DataError(
                        '"rows" must be a point dict or a list of point dicts'
                    )
        elif all(isinstance(p, (list, tuple)) and len(p) > 0 for p in items):
            for p in items:
                v = p[0]
                if isinstance(v, dict):
                    rows.append(v)
                elif isinstance(v, list):
                    rows.extend(v)
                else:
                    raise DataError(
                        "positional upsert params must wrap a point dict "
                        "or a list of point dicts"
                    )
        else:
            raise DataError(
                "upsert executemany needs point dicts, {'rows': ...} sets, "
                "or single-positional [rows] sets — got mixed shapes"
            )
        for row in rows:
            if not isinstance(row, dict):
                raise DataError("upsert rows must be point dicts")
        report = self.connection.client.upsert_many(
            collection, rows, batch_size=100, on_error="stop"
        )
        self._ingest(report, is_executemany=True)
        self.rowcount = len(rows)

    # -- fetching ----------------------------------------------------

    def fetchone(self) -> Optional[Tuple]:
        self._ensure_open()
        if self._pos >= len(self._rows):
            return None
        row = self._rows[self._pos]
        self._pos += 1
        return row

    def fetchmany(self, size: Optional[int] = None) -> List[Tuple]:
        self._ensure_open()
        if size is None:
            size = self.arraysize
        if size <= 0:
            return []
        out = self._rows[self._pos : self._pos + size]
        self._pos += len(out)
        return out

    def fetchall(self) -> List[Tuple]:
        self._ensure_open()
        out = self._rows[self._pos :]
        self._pos = len(self._rows)
        return out

    def nextset(self) -> Optional[bool]:
        """Skip to the next available result set from a multi-statement script.

        Returns True if a new set is available, or None if no more sets exist.
        """
        self._ensure_open()
        self._set_idx += 1
        if self._set_idx >= len(self._result_sets):
            self._rows = []
            self._pos = 0
            self.description = None
            self.rowcount = -1
            return None
        self._rows, self.description, self.rowcount = self._result_sets[self._set_idx]
        self._pos = 0
        return True

    def __iter__(self):  # lazy: yields one row at a time via fetchone
        self._ensure_open()
        return self._iter_rows()

    def _iter_rows(self):
        while True:
            row = self.fetchone()
            if row is None:
                return
            yield row

    # -- excluded surface --------------------------------------------

    def callproc(self, *args: Any, **kwargs: Any) -> Any:
        raise NotSupportedError("callproc is not supported: QQL has no stored procedures")

    def setinputsizes(self, *args: Any, **kwargs: Any) -> None:
        raise NotSupportedError("setinputsizes is not supported")

    def setoutputsize(self, *args: Any, **kwargs: Any) -> None:
        raise NotSupportedError("setoutputsize is not supported")

    # -- lifecycle ---------------------------------------------------

    def close(self) -> None:
        self._closed = True

    def __enter__(self) -> "Cursor":
        return self

    def __exit__(self, *exc_info: Any) -> None:
        self.close()
