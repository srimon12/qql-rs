"""Regression tests for the 0.3.2 live-verdict fixes (P0/P1/P2).

Network-free: everything here either never touches the transport or fails
before the first request (client pointed at an unreachable port).
"""

import unittest

import pyqql
from pyqql import (
    QqlError,
    QqlExecutionError,
    QqlSyntaxError,
    QqlValidationError,
)


class _ArrayLike:
    """Duck-typed numpy array: only exposes ``tolist()``."""

    def __init__(self, values):
        self._values = list(values)

    def tolist(self):
        return list(self._values)


class TestTypedExceptions(unittest.TestCase):
    def test_parse_error_is_typed_and_carries_code(self):
        try:
            pyqql.parse("QUERY FROM")
        except QqlSyntaxError as e:
            self.assertTrue(e.code.startswith("QQL-PARSE"), e.code)
            self.assertEqual(e.kind, "Parse")
        else:
            self.fail("expected QqlSyntaxError")

    def test_builtin_catches_still_work(self):
        # Backward compat: the typed classes subclass the builtin categories.
        with self.assertRaises(SyntaxError):
            pyqql.parse("QUERY FROM")
        with self.assertRaises(ValueError):
            pyqql.parse("QUERY :v FROM docs USING dense")[0].bind({"v": {"bad": "shape"}})

    def test_base_class_catches_everything(self):
        with self.assertRaises(QqlError):
            pyqql.parse("QUERY FROM")


class TestBindingContract(unittest.TestCase):
    def test_null_param_fails_closed(self):
        with self.assertRaises(ValueError) as ctx:
            pyqql.bind("QUERY :v FROM docs USING dense", {"v": None})
        self.assertIn("QQL-BIND-NULL-PARAM", str(ctx.exception))

    def test_array_like_binds_like_a_list(self):
        # numpy / pandas expose tolist(); py_to_json accepts them now.
        bound = pyqql.bind(
            "QUERY :v FROM docs USING dense", {"v": _ArrayLike([0.1, 0.2, 0.3])}
        )
        # The canonical form drops the optional VECTOR keyword (compact
        # vector literal idiom); USING dense re-parses the same AST.
        self.assertEqual(str(bound), "QUERY [0.1, 0.2, 0.3] FROM docs USING dense")

    def test_matrix_param_binds_as_multivector(self):
        bound = pyqql.bind("QUERY :v FROM docs USING dense", {"v": [[0.1, 0.2], [0.3, 0.4]]})
        self.assertIn("[[0.1, 0.2], [0.3, 0.4]]", str(bound))

    def test_explicit_and_implicit_vector_spellings_agree(self):
        explicit = pyqql.parse("QUERY VECTOR :v FROM docs USING dense")[0]
        implicit = pyqql.parse("QUERY :v FROM docs USING dense")[0]
        self.assertEqual(str(explicit), str(implicit))
        params = {"v": [[0.1, 0.2], [0.3, 0.4]]}
        self.assertEqual(str(explicit.bind(params)), str(implicit.bind(params)))
        # compile_query accepts the explicit spelling too (it failed to parse
        # before the fix).
        route = pyqql.compile_query("QUERY VECTOR :v FROM docs USING dense", params)
        self.assertEqual(route["method"], "POST")

    def test_rebinding_a_bound_stmt_raises(self):
        stmt = pyqql.parse("QUERY :v FROM docs LIMIT :lim")[0]
        bound = stmt.bind({"v": [0.1], "lim": 1})
        with self.assertRaises(ValueError) as ctx:
            bound.bind({"v": [0.2], "lim": 2})
        self.assertIn("QQL-BIND-ALREADY-BOUND", str(ctx.exception))
        # bind(None) stays a no-op.
        self.assertEqual(str(bound.bind(None)), str(bound))

    def test_execute_bound_stmt_with_params_raises_before_network(self):
        client = pyqql.Client("http://localhost:1")
        bound = pyqql.parse("QUERY :v FROM docs LIMIT 1")[0].bind({"v": [0.1]})
        with self.assertRaises(ValueError) as ctx:
            client.execute(bound, params={"v": [0.2]})
        self.assertIn("QQL-BIND-ALREADY-BOUND", str(ctx.exception))

    def test_compile_route_on_bound_stmt_with_params_raises(self):
        bound = pyqql.parse("QUERY :v FROM docs LIMIT 1")[0].bind({"v": [0.1]})
        with self.assertRaises(ValueError):
            bound.compile_route(params={"v": [0.2]})


class TestEmptyScriptParity(unittest.TestCase):
    def test_empty_script_raises_like_double_semicolon(self):
        client = pyqql.Client("http://localhost:1")
        with self.assertRaises(ValueError) as ctx:
            client.execute("")
        self.assertIn("QQL-VALIDATION-EMPTY-SCRIPT", str(ctx.exception))
        with self.assertRaises(QqlSyntaxError) as ctx2:
            client.execute(";;")
        self.assertTrue(ctx2.exception.code.startswith("QQL-PARSE"))

    def test_empty_list_raises(self):
        client = pyqql.Client("http://localhost:1")
        with self.assertRaises(ValueError):
            client.execute([])


class TestLimitZero(unittest.TestCase):
    def test_limit_zero_rejected_at_parse_time(self):
        # Live-verified against Qdrant 1.19.1: /points/query answers 422
        # "internal.limit: value 0 invalid, must be 1 or larger" — so QQL
        # rejects LIMIT 0 at parse time instead of shipping a 422.
        self.assertFalse(pyqql.is_valid("QUERY 'x' FROM docs LIMIT 0;"))
        self.assertFalse(pyqql.is_valid("SCROLL FROM docs LIMIT 0;"))

    def test_limit_negative_rejected(self):
        self.assertFalse(pyqql.is_valid("QUERY 'x' FROM docs LIMIT -1;"))


class TestClosedClient(unittest.TestCase):
    def test_close_makes_execute_fail(self):
        client = pyqql.Client("http://localhost:1")
        self.assertFalse(client.is_closed)
        client.close()
        self.assertTrue(client.is_closed)
        with self.assertRaises(RuntimeError) as ctx:
            client.execute("QUERY 'x' FROM docs LIMIT 1")
        self.assertIn("QQL-CLIENT-CLOSED", str(ctx.exception))
        # close() is idempotent.
        client.close()


if __name__ == "__main__":
    unittest.main()


class TestVerdictRoundTwo(unittest.TestCase):
    """N1-N5 from the 1.19.1 verification report."""

    def test_unbound_string_path_fails_closed(self):
        # N2: execute(str) with no params used to ship the raw placeholder and
        # get a 422 from the server; the plan gate now raises the binder's own
        # missing-param error before any request leaves.
        client = pyqql.Client("http://localhost:1")
        with self.assertRaises(ValueError) as ctx:
            client.execute("QUERY VECTOR :qvec FROM docs LIMIT 1")
        self.assertIn("QQL-BIND-MISSING-PARAM", str(ctx.exception))

    def test_formula_datetime_binds_on_the_prepared_path(self):
        # N3: TARGET = :now with an ISO string used to lose the type through
        # the AST (bare identifier == DEFAULTS key) and the server answered
        # "Expected number value ..."; the prepared path now produces the
        # inline datetime form.
        stmt = pyqql.parse(
            "QUERY FORMULA GAUSS_DECAY(DATETIME_KEY('judgment_date'), TARGET = :now) FROM docs"
        )[0]
        bound = stmt.bind({"now": "2024-01-01T00:00:00Z"})
        self.assertIn("TARGET = datetime('2024-01-01T00:00:00Z')", str(bound))
        route = bound.compile_route()
        self.assertIn("2024-01-01T00:00:00Z", str(route["payload"]))

    def test_transport_error_carries_request_id(self):
        # N4: the request id is a structured attribute, not message-only.
        client = pyqql.Client("http://localhost:1")
        try:
            client.execute("QUERY 'x' FROM docs LIMIT 1")
        except pyqql.QqlTransportError as e:
            self.assertIsInstance(e.request_id, str)
            self.assertTrue(e.request_id.startswith("qql-"), e.request_id)
            self.assertIn("request_id", e.fields)
            self.assertIn("url", e.fields)
        else:
            self.fail("expected QqlTransportError")

    def test_default_keys_stay_variables_when_binding(self):
        # N3 sibling: bare identifiers are DEFAULTS keys, never params.
        stmt = pyqql.parse(
            "QUERY FORMULA GAUSS_DECAY(rank, TARGET = 100.0, SCALE = 10.0) DEFAULTS (rank = 0.0) FROM docs"
        )[0]
        bound = stmt.bind({"rank": 5})
        self.assertIn("GAUSS_DECAY(rank", str(bound))
