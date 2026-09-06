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
    def test_limit_zero_is_valid(self):
        self.assertTrue(pyqql.is_valid("QUERY 'x' FROM docs LIMIT 0;"))
        self.assertTrue(pyqql.is_valid("SCROLL FROM docs LIMIT 0;"))

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
