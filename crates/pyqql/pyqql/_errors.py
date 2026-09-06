"""Typed exception hierarchy shared byte-identically by the ``pyqql`` and
``pyqql-edge`` Python wrappers.

Every error raised by the native module is an instance of one of these
classes and carries the stable QQL error code, the error kind, and the
source span — so programs can handle failures by ``code`` instead of
string-matching messages.

The classes also subclass the builtin exception each category used to
raise (``SyntaxError`` / ``ValueError`` / ``RuntimeError``), so existing
``except`` clauses keep working.

A CI check diffs the two copies of this file, so edit both or neither
(they must stay in lockstep with ``_dx_report.py``'s sharing model).
"""

from typing import Optional, Tuple


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
    ) -> None:
        super().__init__(message)
        self.message = message
        self.code = code
        self.kind = kind
        self.span = span


class QqlSyntaxError(QqlError, SyntaxError):
    """Lexer / parser errors (``QQL-LEX-*``, ``QQL-PARSE-*``)."""


class QqlValidationError(QqlError, ValueError):
    """Validation and parameter-binding errors (``QQL-BIND-*``,
    ``QQL-VALIDATION-*``, ``QQL-PLAN-*``)."""


class QqlExecutionError(QqlError, RuntimeError):
    """Execution-time errors (batch invariants, closed clients, …)."""


class QqlTransportError(QqlError, RuntimeError):
    """Network / timeout errors (``QQL-TRANSPORT``, ``QQL-TIMEOUT``)."""


class QqlBackendError(QqlError, RuntimeError):
    """Errors reported by Qdrant itself (``QQL-BACKEND-*``, ``QQL-GRPC-*``)."""
