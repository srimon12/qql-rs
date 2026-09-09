"""Single exception hierarchy shared byte-identically by the ``pyqql`` and
``pyqql-edge`` Python wrappers (PEP 249 names and native ``Qql*`` names are
one tree, not two — the old ``raise X from Y`` translation layer is gone).

Every error raised by the native module is an instance of one of the
``Qql*`` classes below, and each ``Qql*`` class already *is* the DB-API
error its failures used to be translated into — so ``except
ProgrammingError`` / ``except OperationalError`` / ``except QqlError``
all catch native failures directly, with ``code`` / ``kind`` / ``span`` /
``fields`` intact on the original object (no re-wrap, no ``__cause__``
chain for native failures).

Layout (``Error`` is an alias of ``QqlError``, one root with two names)::

    QqlError (= Error)
    ├── Warning is NOT here (PEP 249 keeps warnings outside Error)
    ├── InterfaceError(Error) ............. closed handles, wrapper misuse
    ├── DatabaseError(Error)
    │   ├── DataError .................... malformed rows / params data
    │   ├── OperationalError ............. runtime / transport / backend
    │   ├── IntegrityError ............... reserved (compat)
    │   ├── InternalError ................ reserved (compat)
    │   ├── ProgrammingError ............. bad QQL text / binding calls
    │   └── NotSupportedError ............ optional surface QQL cannot honor
    ├── QqlSyntaxError(ProgrammingError, SyntaxError)
    ├── QqlValidationError(ProgrammingError, DataError, ValueError)
    ├── QqlExecutionError(OperationalError, RuntimeError)
    ├── QqlTransportError(OperationalError, RuntimeError)
    └── QqlBackendError(OperationalError, RuntimeError)

Per-class parent choices:

* ``Error = QqlError`` is an *alias*, not a subclass: PEP 249's ``Error``
  (root of all DB-API errors) and ``QqlError`` (root of all QQL errors)
  name the same concept, so they are the same object and
  ``except QqlError`` keeps catching everything DB-API.
* ``Warning`` stays a plain ``Exception`` per PEP 249 — warnings are not
  failures, so ``except QqlError`` must not catch them.
* ``InterfaceError`` hangs directly off ``Error`` (sibling of
  ``DatabaseError``), exactly as PEP 249 draws it: a closed handle is an
  interface problem, not a database failure. (The previous hierarchy had
  it under ``DatabaseError``; that was a spec deviation.)
* ``QqlSyntaxError`` → ``ProgrammingError``: bad QQL text is the textbook
  programming error. The ``SyntaxError`` mixin stays so old
  ``except SyntaxError`` clauses keep working.
* ``QqlValidationError`` → ``ProgrammingError`` *and* ``DataError``: the
  single native class spans both PEP 249 meanings and cannot be split
  per-code without reintroducing translation. Bind-arity/shape problems
  (``QQL-BIND-MISSING-PARAM``, ``QQL-BIND-BATCH-LENGTH``,
  ``QQL-BIND-INVALID-PARAMS``, …) are programming errors per the sqlite3
  binding precedent; value problems (``QQL-BIND-TYPE-MISMATCH`` including
  non-finite floats, ``QQL-BIND-NULL-PARAM``) are data errors per PEP 249
  ("numeric value out of range" analogue). Multiple inheritance keeps
  both ``except`` styles working; the pre-change behavior
  (``except ProgrammingError`` catches every validation failure) is
  preserved. The ``ValueError`` mixin stays for old ``except ValueError``
  clauses.
* ``QqlTransportError`` / ``QqlBackendError`` → ``OperationalError``:
  network/timeout failures and Qdrant-side rejections are "related to the
  database's operation and not necessarily under the control of the
  programmer" (PEP 249). This also preserves the previous translation
  table (both mapped to ``OperationalError``), so retry-on-
  ``OperationalError`` logic keeps catching live server errors.
* ``QqlExecutionError`` → ``OperationalError``: batch-invariant failures
  are runtime operation failures. The one exception is
  ``QQL-CLIENT-CLOSED`` (handle misuse → ``InterfaceError`` per PEP 249),
  which the DB-API cursor refines with a narrow code check because the
  Rust kind system has no interface kind. The ``RuntimeError`` mixins
  stay so old ``except RuntimeError`` clauses keep working.

A CI check diffs the two copies of this file, so edit both or neither
(they must stay in lockstep with ``_dx_report.py``'s sharing model).
"""

from typing import Dict, Optional, Tuple

__all__ = [
    "QqlError",
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
    "QqlSyntaxError",
    "QqlValidationError",
    "QqlExecutionError",
    "QqlTransportError",
    "QqlBackendError",
]


class QqlError(Exception):
    """Base class for every error raised by pyqql / pyqql-edge.

    Attributes:
        message: Full formatted message (also the ``str()`` of the error).
        code: Stable error code (e.g. ``"QQL-BIND-NULL-PARAM"``), when known.
        kind: Error kind (``"Lex"``, ``"Parse"``, ``"Validation"``,
            ``"Execution"``, ``"Transport"``, ``"Backend"``), when known.
        span: ``(start, end)`` byte offsets into the source, when known.
    """

    def __init__(
        self,
        message: str,
        code: Optional[str] = None,
        kind: Optional[str] = None,
        span: Optional[Tuple[int, int]] = None,
        fields: Optional[Dict[str, str]] = None,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.code = code
        self.kind = kind
        self.span = span
        self.fields: Dict[str, str] = dict(fields) if fields else {}
        # Convenience attribute: programmatic request correlation never
        # requires message parsing.
        self.request_id = self.fields.get("request_id")


# PEP 249 root name. Alias (not subclass): one root, two spellings.
Error = QqlError


class Warning(Exception):
    """DB-API warning (not an error; intentionally outside QqlError)."""


class DatabaseError(Error):
    """Base for data-path failures (has QQL ``code`` / ``kind`` / ``span``)."""


class InterfaceError(Error):
    """Misuse of a closed handle or the wrapper itself (PEP 249: sibling of
    DatabaseError, not a database failure)."""


class DataError(DatabaseError):
    """Malformed row/parameter data (e.g. bad ``executemany`` rows)."""


class OperationalError(DatabaseError):
    """Execution, transport, or backend failure at runtime."""


class IntegrityError(DatabaseError):
    """Constraint violation (reserved; Qdrant reports these as backend errors)."""


class InternalError(DatabaseError):
    """Internal driver failure (reserved for compat)."""


class ProgrammingError(DatabaseError):
    """Bad QQL text or parameter binding (syntax / validation errors)."""


class NotSupportedError(DatabaseError):
    """Optional DB-API surface QQL cannot honor (transactions, procs)."""


class QqlSyntaxError(ProgrammingError, SyntaxError):
    """Lexer / parser errors (``QQL-LEX-*``, ``QQL-PARSE-*``)."""


class QqlValidationError(ProgrammingError, DataError, ValueError):
    """Validation and parameter-binding errors (``QQL-BIND-*``,
    ``QQL-VALIDATION-*``, ``QQL-PLAN-*``)."""


class QqlExecutionError(OperationalError, RuntimeError):
    """Execution-time errors (batch invariants, closed clients, …)."""


class QqlTransportError(OperationalError, RuntimeError):
    """Network / timeout errors (``QQL-TRANSPORT``, ``QQL-TIMEOUT``)."""


class QqlBackendError(OperationalError, RuntimeError):
    """Errors reported by Qdrant itself (``QQL-BACKEND-*``, ``QQL-GRPC-*``)."""
