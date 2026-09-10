import types
import unittest
import urllib.request
import uuid

import pyqql


def _qdrant_available():
    """Check if Qdrant is reachable at localhost:6333."""
    try:
        req = urllib.request.Request("http://localhost:6333/healthz")
        with urllib.request.urlopen(req, timeout=2) as resp:
            return resp.status == 200
    except Exception:
        return False


_qdrant_ok = _qdrant_available()


def _point_report(rows, operation="QUERY", message=None):
    return {
        "ok": True,
        "results": [
            {
                "ok": True,
                "operation": operation,
                "message": message or f"Found {len(rows)} hits",
                "data": rows,
            }
        ],
        "succeeded": 1,
        "failed": 0,
    }


def _count_report(n):
    return {
        "ok": True,
        "results": [
            {
                "ok": True,
                "operation": "COUNT",
                "message": f"Count: {n}",
                "data": {"count": n},
            }
        ],
        "succeeded": 1,
        "failed": 0,
    }


def _facet_report(pairs):
    return {
        "ok": True,
        "results": [
            {
                "ok": True,
                "operation": "FACET",
                "message": f"Found {len(pairs)} facet hit(s)",
                "data": [{"value": v, "count": n} for v, n in pairs],
            }
        ],
        "succeeded": 1,
        "failed": 0,
    }


def _names_report(names):
    return {
        "ok": True,
        "results": [
            {
                "ok": True,
                "operation": "SHOW_COLLECTIONS",
                "message": f"Found {len(names)} collection(s)",
                "data": {"result": {"collections": [{"name": n} for n in names]}},
            }
        ],
        "succeeded": 1,
        "failed": 0,
    }


def _groups_report(groups):
    return {
        "ok": True,
        "results": [
            {
                "ok": True,
                "operation": "QUERY_GROUPS",
                "message": f"Found {len(groups)} group(s)",
                "data": {"result": {"groups": groups}},
            }
        ],
        "succeeded": 1,
        "failed": 0,
    }


def _ddl_report(operation="CREATE_COLLECTION"):
    return {
        "ok": True,
        "results": [
            {"ok": True, "operation": operation, "message": f"{operation} ok"}
        ],
        "succeeded": 1,
        "failed": 0,
    }


class FakeClient:
    """Offline stand-in for pyqql.Client (records calls, replays reports)."""

    def __init__(self, reports=None, error=None):
        self.calls = []
        self.reports = list(reports or [])
        self.error = error
        self.closed = False

    def execute(self, query, params=None, on_error="stop"):
        self.calls.append(("execute", query, params, on_error))
        if self.error is not None:
            raise self.error
        if self.reports:
            return self.reports.pop(0)
        return {"ok": True, "results": [], "succeeded": 0, "failed": 0}

    def upsert_many(self, collection, rows, batch_size=100, on_error="stop"):
        self.calls.append(("upsert_many", collection, list(rows), batch_size, on_error))
        if self.error is not None:
            raise self.error
        n = len(rows)
        return {
            "ok": True,
            "results": [
                {
                    "ok": True,
                    "operation": "UPSERT",
                    "message": f"Upserted {n} point(s)",
                    "data": {"count": n},
                }
            ],
            "succeeded": 1,
            "failed": 0,
        }

    def close(self):
        self.closed = True


POINTS = [
    {"id": 1, "score": 0.95, "payload": {"tag": "a"}},
    {"id": "uuid-1", "score": 0.82, "payload": {"tag": "b"}},
]


class TestDbapiModule(unittest.TestCase):
    def test_module_globals(self):
        self.assertEqual(pyqql.apilevel, "2.0")
        self.assertEqual(pyqql.threadsafety, 1)
        self.assertEqual(pyqql.paramstyle, "named")

    def test_exception_hierarchy_no_fork(self):
        # Single hierarchy: every DB-API name lives under QqlError (defined
        # in _errors.py, no translation layer). PEP 249 shape: InterfaceError
        # is a sibling of DatabaseError (both directly under Error), the
        # rest hang off DatabaseError.
        for name in (
            "DatabaseError",
            "DataError",
            "OperationalError",
            "IntegrityError",
            "InternalError",
            "ProgrammingError",
            "NotSupportedError",
        ):
            cls = getattr(pyqql, name)
            self.assertTrue(
                issubclass(cls, pyqql.DatabaseError), f"{name} not a DatabaseError"
            )
            self.assertTrue(
                issubclass(cls, pyqql.QqlError), f"{name} forked outside QqlError"
            )
        self.assertTrue(issubclass(pyqql.InterfaceError, pyqql.Error))
        self.assertTrue(issubclass(pyqql.InterfaceError, pyqql.QqlError))
        self.assertFalse(
            issubclass(pyqql.InterfaceError, pyqql.DatabaseError),
            "InterfaceError must be a sibling of DatabaseError (PEP 249)",
        )
        self.assertIs(pyqql.Error, pyqql.QqlError)
        self.assertTrue(issubclass(pyqql.Warning, Exception))
        self.assertFalse(
            issubclass(pyqql.Warning, pyqql.Error),
            "Warning is not an error (PEP 249)",
        )
        # Native errors ARE the DB-API errors (re-parented, never translated).
        self.assertTrue(issubclass(pyqql.QqlSyntaxError, pyqql.ProgrammingError))
        self.assertTrue(issubclass(pyqql.QqlValidationError, pyqql.ProgrammingError))
        # Validation value problems (QQL-BIND-TYPE-MISMATCH, ...) are also
        # data errors; arity/shape problems stay programming errors via the
        # shared ProgrammingError parent (see _errors.py).
        self.assertTrue(issubclass(pyqql.QqlValidationError, pyqql.DataError))
        self.assertTrue(issubclass(pyqql.QqlTransportError, pyqql.OperationalError))
        self.assertTrue(issubclass(pyqql.QqlBackendError, pyqql.OperationalError))
        self.assertTrue(issubclass(pyqql.QqlExecutionError, pyqql.OperationalError))

    def test_connect_wraps_client_kwargs(self):
        conn = pyqql.connect(url="http://localhost:6333")
        try:
            self.assertIsInstance(conn, pyqql.Connection)
            self.assertIsInstance(conn.client, pyqql.Client)
        finally:
            conn.close()

    def test_connect_positional_url(self):
        conn = pyqql.connect("http://localhost:6333")
        try:
            self.assertIsInstance(conn, pyqql.Connection)
            self.assertIsInstance(conn.client, pyqql.Client)
        finally:
            conn.close()

    def test_connect_rejects_client_plus_args(self):
        with self.assertRaises(TypeError):
            pyqql.Connection(client=FakeClient(), url="http://localhost:6333")


class TestDbapiCursorOffline(unittest.TestCase):
    def test_nextset_multi_statement(self):
        report = pyqql.ExecutionReport({
            "ok": True,
            "results": [
                {
                    "ok": True,
                    "operation": "QUERY",
                    "message": "Found 2 hits",
                    "data": POINTS,
                },
                {
                    "ok": True,
                    "operation": "COUNT",
                    "message": "Found 42 points",
                    "data": {"count": 42},
                }
            ],
            "succeeded": 2,
            "failed": 0,
        })
        conn = pyqql.Connection(client=FakeClient(reports=[report]))
        cur = conn.cursor()
        cur.execute("QUERY [0.1] FROM docs; COUNT FROM docs;")
        
        # Set 0: QUERY
        self.assertEqual([d[0] for d in cur.description], ["id", "score", "payload"])
        rows0 = cur.fetchall()
        self.assertEqual(len(rows0), 2)
        
        # Advance to Set 1: COUNT
        self.assertTrue(cur.nextset())
        self.assertEqual([d[0] for d in cur.description], ["count"])
        rows1 = cur.fetchall()
        self.assertEqual(rows1, [(42,)])
        
        # No more sets
        self.assertIsNone(cur.nextset())
        self.assertIsNone(cur.description)
        self.assertEqual(cur.fetchall(), [])

    def test_execute_fetch_flow(self):
        conn = pyqql.Connection(client=FakeClient(reports=[_point_report(POINTS)]))
        cur = conn.cursor()
        self.assertIs(cur.execute("QUERY [0.1] FROM docs LIMIT 5"), cur)
        self.assertEqual(
            [d[0] for d in cur.description], ["id", "score", "payload"]
        )
        for col in cur.description:
            self.assertEqual(len(col), 7)
        self.assertEqual(cur.rowcount, 2)
        first = cur.fetchone()
        self.assertEqual(first, (1, 0.95, {"tag": "a"}))
        self.assertIsInstance(first[2], dict)
        rest = cur.fetchall()
        self.assertEqual(rest, [("uuid-1", 0.82, {"tag": "b"})])
        self.assertIsNone(cur.fetchone())

    def test_fetchmany_arraysize(self):
        conn = pyqql.Connection(client=FakeClient(reports=[_point_report(POINTS)]))
        cur = conn.cursor()
        cur.execute("QUERY [0.1] FROM docs LIMIT 5")
        self.assertEqual(cur.arraysize, 1)
        self.assertEqual(len(cur.fetchmany()), 1)
        cur.arraysize = 10
        self.assertEqual(len(cur.fetchmany()), 1)
        self.assertEqual(cur.fetchmany(), [])
        self.assertEqual(cur.fetchmany(size=0), [])
        self.assertEqual(cur.fetchmany(size=-1), [])

    def test_iter_yields_lazily(self):
        conn = pyqql.Connection(client=FakeClient(reports=[_point_report(POINTS)]))
        cur = conn.cursor()
        cur.execute("QUERY [0.1] FROM docs LIMIT 5")
        it = iter(cur)
        self.assertIsInstance(it, types.GeneratorType)
        self.assertEqual(list(it), [(1, 0.95, {"tag": "a"}), ("uuid-1", 0.82, {"tag": "b"})])
        self.assertEqual(list(cur), [])

    def test_description_with_vector(self):
        rows = [
            {"id": 7, "score": 1.0, "payload": {"t": "x"}, "vector": [0.1, 0.2]},
            {"id": 8, "payload": None},
        ]
        conn = pyqql.Connection(client=FakeClient(reports=[_point_report(rows)]))
        cur = conn.cursor()
        cur.execute("QUERY [0.1, 0.2] FROM docs WITH VECTOR LIMIT 5")
        self.assertEqual(
            [d[0] for d in cur.description], ["id", "score", "payload", "vector"]
        )
        fetched = cur.fetchall()
        self.assertEqual(fetched[0], (7, 1.0, {"t": "x"}, [0.1, 0.2]))
        self.assertEqual(fetched[1], (8, 0.0, {}, None))
        columns = [d[0] for d in cur.description]
        as_dicts = [dict(zip(columns, row)) for row in fetched]
        self.assertEqual(as_dicts[0]["payload"], {"t": "x"})

    def test_empty_hits_keep_description(self):
        conn = pyqql.Connection(client=FakeClient(reports=[_point_report([])]))
        cur = conn.cursor()
        cur.execute("QUERY [0.1] FROM docs LIMIT 5")
        self.assertEqual([d[0] for d in cur.description], ["id", "score", "payload"])
        self.assertEqual(cur.rowcount, 0)
        self.assertEqual(cur.fetchall(), [])

    def test_count_rows(self):
        conn = pyqql.Connection(client=FakeClient(reports=[_count_report(42)]))
        cur = conn.cursor()
        cur.execute("COUNT FROM docs")
        self.assertEqual([d[0] for d in cur.description], ["count"])
        self.assertEqual(cur.fetchall(), [(42,)])
        self.assertEqual(cur.rowcount, 1)

    def test_facet_rows(self):
        conn = pyqql.Connection(
            client=FakeClient(reports=[_facet_report([("tech", 10), ("news", 4)])])
        )
        cur = conn.cursor()
        cur.execute("FACET category FROM docs LIMIT 10")
        self.assertEqual([d[0] for d in cur.description], ["value", "count"])
        self.assertEqual(cur.fetchall(), [("tech", 10), ("news", 4)])
        self.assertEqual(cur.rowcount, 2)

    def test_show_collections_rows(self):
        conn = pyqql.Connection(
            client=FakeClient(reports=[_names_report(["docs", "sec10k"])])
        )
        cur = conn.cursor()
        cur.execute("SHOW COLLECTIONS")
        self.assertEqual([d[0] for d in cur.description], ["name"])
        self.assertEqual(cur.fetchall(), [("docs",), ("sec10k",)])
        self.assertEqual(cur.rowcount, 2)

    def test_groups_rows(self):
        groups = [
            {"id": "a", "hits": [{"id": 1, "score": 0.9}]},
            {"id": "b", "hits": []},
        ]
        conn = pyqql.Connection(client=FakeClient(reports=[_groups_report(groups)]))
        cur = conn.cursor()
        cur.execute("QUERY 'x' FROM docs GROUP BY category LIMIT 5")
        self.assertEqual([d[0] for d in cur.description], ["group_id", "hits"])
        self.assertEqual(
            cur.fetchall(), [("a", [{"id": 1, "score": 0.9}]), ("b", [])]
        )
        self.assertEqual(cur.rowcount, 2)

    def test_ddl_has_no_result_set(self):
        conn = pyqql.Connection(client=FakeClient(reports=[_ddl_report()]))
        cur = conn.cursor()
        cur.execute("CREATE COLLECTION docs")
        self.assertIsNone(cur.description)
        self.assertEqual(cur.rowcount, -1)
        self.assertIsNone(cur.fetchone())
        self.assertEqual(cur.fetchall(), [])

    def test_fetch_before_execute(self):
        cur = pyqql.Connection(client=FakeClient()).cursor()
        self.assertIsNone(cur.description)
        self.assertEqual(cur.rowcount, -1)
        self.assertIsNone(cur.fetchone())
        self.assertEqual(cur.fetchall(), [])

    def test_execute_passes_params_and_normalizes_tuples(self):
        fake = FakeClient(reports=[_count_report(1)])
        cur = pyqql.Connection(client=fake).cursor()
        cur.execute("QUERY :v FROM docs LIMIT 1", params={"v": (0.1, 0.2)})
        kind, query, params, on_error = fake.calls[0]
        self.assertEqual(kind, "execute")
        self.assertEqual(params, {"v": [0.1, 0.2]})

    def test_failed_result_raises(self):
        bad = {
            "ok": False,
            "results": [
                {
                    "ok": False,
                    "operation": "BACKEND",
                    "message": "boom",
                    "data": None,
                }
            ],
            "succeeded": 0,
            "failed": 1,
        }
        cur = pyqql.Connection(client=FakeClient(reports=[bad])).cursor()
        with self.assertRaises(pyqql.OperationalError):
            cur.execute("QUERY [0.1] FROM docs LIMIT 1")


class TestDbapiExecutemanyOffline(unittest.TestCase):
    SQL = "UPSERT INTO docs VALUES :rows"
    ROWS = [
        {"id": 1, "vector": {"dense": [0.1, 0.2]}, "tag": "a"},
        {"id": 2, "vector": {"dense": [0.3, 0.4]}, "tag": "b"},
    ]

    def test_upsert_rows_delegates_to_upsert_many(self):
        fake = FakeClient()
        cur = pyqql.Connection(client=fake).cursor()
        cur.executemany(self.SQL, self.ROWS)
        self.assertEqual(len(fake.calls), 1)
        kind, collection, rows, batch_size, on_error = fake.calls[0]
        self.assertEqual(kind, "upsert_many")
        self.assertEqual(collection, "docs")
        self.assertEqual(rows, self.ROWS)
        self.assertEqual(cur.rowcount, 2)
        self.assertIsNone(cur.description)
        self.assertEqual(cur.fetchall(), [])

    def test_upsert_rows_param_sets(self):
        fake = FakeClient()
        cur = pyqql.Connection(client=fake).cursor()
        cur.executemany(
            self.SQL, [{"rows": self.ROWS[0]}, {"rows": [self.ROWS[1]]}]
        )
        _, _, rows, _, _ = fake.calls[0]
        self.assertEqual(rows, self.ROWS)
        self.assertEqual(cur.rowcount, 2)

    def test_upsert_positional_param_sets(self):
        fake = FakeClient()
        cur = pyqql.Connection(client=fake).cursor()
        cur.executemany(self.SQL, [[self.ROWS[0]], [self.ROWS[1]]])
        _, _, rows, _, _ = fake.calls[0]
        self.assertEqual(rows, self.ROWS)

    def test_upsert_accepts_parsed_stmt(self):
        fake = FakeClient()
        stmt = pyqql.parse(self.SQL)[0]
        cur = pyqql.Connection(client=fake).cursor()
        cur.executemany(stmt, self.ROWS)
        self.assertEqual(fake.calls[0][0], "upsert_many")
        self.assertEqual(fake.calls[0][1], "docs")

    def test_upsert_mixed_shapes_raise_data_error(self):
        cur = pyqql.Connection(client=FakeClient()).cursor()
        with self.assertRaises(pyqql.DataError):
            cur.executemany(self.SQL, [self.ROWS[0], {"rows": self.ROWS[1]}])
        with self.assertRaises(pyqql.DataError):
            cur.executemany(self.SQL, ["not-a-point"])

    def test_non_upsert_runs_statement_scoped_batch(self):
        combined = {
            "ok": True,
            "results": [
                {
                    "ok": True,
                    "operation": "COUNT",
                    "message": "Count: 3",
                    "data": {"count": 3},
                },
                {
                    "ok": True,
                    "operation": "COUNT",
                    "message": "Count: 1",
                    "data": {"count": 1},
                },
            ],
            "succeeded": 2,
            "failed": 0,
        }
        fake = FakeClient(reports=[combined])
        cur = pyqql.Connection(client=fake).cursor()
        sql = "COUNT FROM docs WHERE tag = :t"
        cur.executemany(sql, [{"t": "a"}, {"t": "b"}])
        kind, query, params, _ = fake.calls[0]
        self.assertEqual(kind, "execute")
        self.assertEqual(query, [sql, sql])
        self.assertEqual(params, [{"t": "a"}, {"t": "b"}])
        self.assertEqual(cur.fetchall(), [(3,), (1,)])
        self.assertEqual(cur.rowcount, 2)

    def test_executemany_empty_is_noop(self):
        fake = FakeClient()
        cur = pyqql.Connection(client=fake).cursor()
        cur.executemany(self.SQL, [])
        self.assertEqual(fake.calls, [])
        self.assertEqual(cur.rowcount, 0)
        self.assertEqual(cur.fetchall(), [])

    def test_executemany_rejects_multi_statement(self):
        fake = FakeClient()
        cur = pyqql.Connection(client=fake).cursor()
        with self.assertRaises(pyqql.ProgrammingError):
            cur.executemany("COUNT FROM a; COUNT FROM b", [{}, {}])
        self.assertEqual(fake.calls, [])

    def test_executemany_rejects_bare_mapping(self):
        cur = pyqql.Connection(client=FakeClient()).cursor()
        with self.assertRaises(pyqql.ProgrammingError):
            cur.executemany("COUNT FROM docs", {"t": "a"})


class TestDbapiErrorsOffline(unittest.TestCase):
    def test_syntax_maps_to_programming_error(self):
        err = pyqql.QqlSyntaxError("bad", code="QQL-PARSE-X", kind="Parse", span=(0, 3))
        cur = pyqql.Connection(client=FakeClient(error=err)).cursor()
        with self.assertRaises(pyqql.ProgrammingError) as ctx:
            cur.execute("BROKEN !!!")
        # Single hierarchy: no translation, the identical object propagates.
        self.assertIs(ctx.exception, err)
        self.assertEqual(ctx.exception.code, "QQL-PARSE-X")
        self.assertEqual(ctx.exception.span, (0, 3))
        self.assertIsInstance(ctx.exception, pyqql.QqlError)

    def test_validation_maps_to_programming_error(self):
        err = pyqql.QqlValidationError("bad bind", code="QQL-BIND-X", kind="Validation")
        cur = pyqql.Connection(client=FakeClient(error=err)).cursor()
        with self.assertRaises(pyqql.ProgrammingError) as ctx:
            cur.execute("QUERY :v FROM docs", params={})
        self.assertIs(ctx.exception, err)
        # Validation value problems are data errors too (see _errors.py).
        self.assertIsInstance(ctx.exception, pyqql.DataError)

    def test_transport_and_backend_map_to_operational_error(self):
        for err in (
            pyqql.QqlTransportError("down", code="QQL-TRANSPORT", kind="Transport"),
            pyqql.QqlBackendError("rejected", code="QQL-BACKEND-X", kind="Backend"),
        ):
            with self.subTest(err=err.code):
                cur = pyqql.Connection(client=FakeClient(error=err)).cursor()
                with self.assertRaises(pyqql.OperationalError) as ctx:
                    cur.execute("COUNT FROM docs")
                self.assertIs(ctx.exception, err)
                self.assertEqual(ctx.exception.code, err.code)

    def test_closed_client_maps_to_interface_error(self):
        err = pyqql.QqlExecutionError(
            "client is closed", code="QQL-CLIENT-CLOSED", kind="Execution"
        )
        cur = pyqql.Connection(client=FakeClient(error=err)).cursor()
        with self.assertRaises(pyqql.InterfaceError) as ctx:
            cur.execute("COUNT FROM docs")
        # The one remaining remap: handle misuse is InterfaceError (PEP 249)
        # even though the native kind system reports it as execution. Code
        # and fields are carried over; the native error stays as __cause__.
        self.assertEqual(ctx.exception.code, "QQL-CLIENT-CLOSED")
        self.assertIs(ctx.exception.__cause__, err)
        self.assertIsInstance(ctx.exception, pyqql.QqlError)

    def test_foreign_errors_propagate_unwrapped(self):
        # Translation layer removed: only QqlError failures are part of the
        # DB-API contract. A foreign exception from the client (a host bug,
        # not a QQL failure) surfaces untouched instead of being masked as
        # ProgrammingError/OperationalError via `raise X from Y`.
        err = ValueError("nope")
        cur = pyqql.Connection(client=FakeClient(error=err)).cursor()
        with self.assertRaises(ValueError) as ctx:
            cur.execute("COUNT FROM docs", params={"a": 1})
        self.assertIs(ctx.exception, err)
        self.assertNotIsInstance(ctx.exception, pyqql.Error)

    def test_genuine_syntax_error_offline(self):
        # Parsing happens client-side: no server round-trip needed.
        conn = pyqql.connect(url="http://localhost:6333")
        try:
            with self.assertRaises(pyqql.ProgrammingError) as ctx:
                conn.cursor().execute("SELECT * FROM docs")
            self.assertIsNotNone(ctx.exception.code)
        finally:
            conn.close()

    def test_native_syntax_error_is_programming_error_without_cursor(self):
        # Single hierarchy: no cursor and no translation involved — the
        # native QqlSyntaxError from client.execute already satisfies the
        # DB-API `except ProgrammingError` clause by inheritance. Parsing
        # fails client-side, so no server round-trip happens.
        client = pyqql.Client("http://localhost:1")
        try:
            with self.assertRaises(pyqql.ProgrammingError) as ctx:
                client.execute("SELECT * FROM docs")
            self.assertIs(type(ctx.exception), pyqql.QqlSyntaxError)
            self.assertIsNotNone(ctx.exception.code)
            self.assertIsNone(ctx.exception.__cause__)
        finally:
            client.close()

    def test_qql_error_catches_everything(self):
        # `except QqlError` keeps catching every error in the tree (native
        # and DB-API-synthesized alike). Warning is the only name outside.
        errs = [
            pyqql.QqlSyntaxError("s", code="QQL-PARSE-X", kind="Parse"),
            pyqql.QqlValidationError("v", code="QQL-BIND-X", kind="Validation"),
            pyqql.QqlExecutionError("e", code="QQL-CLIENT-CLOSED", kind="Execution"),
            pyqql.QqlTransportError("t", code="QQL-TRANSPORT", kind="Transport"),
            pyqql.QqlBackendError("b", code="QQL-BACKEND-X", kind="Backend"),
            pyqql.InterfaceError("closed"),
            pyqql.DatabaseError("d"),
            pyqql.DataError("data"),
            pyqql.OperationalError("op"),
            pyqql.IntegrityError("integrity"),
            pyqql.InternalError("internal"),
            pyqql.ProgrammingError("prog"),
            pyqql.NotSupportedError("unsupported"),
        ]
        for err in errs:
            with self.subTest(err=type(err).__name__):
                try:
                    raise err
                except pyqql.QqlError:
                    pass
                else:
                    self.fail(f"{type(err).__name__} escaped except QqlError")
                self.assertIsInstance(err, pyqql.Error)
        self.assertFalse(issubclass(pyqql.Warning, pyqql.QqlError))

    def test_excluded_surface(self):
        cur = pyqql.Connection(client=FakeClient()).cursor()
        with self.assertRaises(pyqql.NotSupportedError):
            cur.callproc("p")
        with self.assertRaises(pyqql.NotSupportedError):
            cur.setinputsizes([1])
        with self.assertRaises(pyqql.NotSupportedError):
            cur.setoutputsize(100)
        # No base class defines these: they come straight from Cursor.
        for name in ("callproc", "setinputsizes", "setoutputsize"):
            self.assertIn(name, pyqql.Cursor.__dict__)

    def test_rollback_raises_commit_noops(self):
        conn = pyqql.Connection(client=FakeClient())
        self.assertIsNone(conn.commit())
        with self.assertRaises(pyqql.NotSupportedError):
            conn.rollback()
        conn.close()


class TestDbapiLifecycleOffline(unittest.TestCase):
    def test_close_delegates_and_is_idempotent(self):
        fake = FakeClient()
        conn = pyqql.Connection(client=fake)
        conn.close()
        self.assertTrue(fake.closed)
        conn.close()

    def test_operations_on_closed_cursor_raise(self):
        conn = pyqql.Connection(client=FakeClient(reports=[_count_report(1)]))
        cur = conn.cursor()
        cur.close()
        with self.assertRaises(pyqql.InterfaceError):
            cur.execute("COUNT FROM docs")
        with self.assertRaises(pyqql.InterfaceError):
            cur.fetchone()
        with self.assertRaises(pyqql.InterfaceError):
            cur.fetchall()
        with self.assertRaises(pyqql.InterfaceError):
            iter(cur)

    def test_operations_after_connection_close_raise(self):
        conn = pyqql.Connection(client=FakeClient(reports=[_count_report(1)]))
        cur = conn.cursor()
        conn.close()
        with self.assertRaises(pyqql.InterfaceError):
            cur.execute("COUNT FROM docs")
        with self.assertRaises(pyqql.InterfaceError):
            conn.cursor()

    def test_context_managers(self):
        with pyqql.Connection(client=FakeClient(reports=[_count_report(2)])) as conn:
            with conn.cursor() as cur:
                cur.execute("COUNT FROM docs")
                self.assertEqual(cur.fetchall(), [(2,)])
        self.assertTrue(conn._closed)
        self.assertTrue(cur._closed)


@unittest.skipUnless(_qdrant_ok, "Qdrant not available at localhost:6333")
class TestDbapiLive(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.collection = f"tmp_pyqql_dbapi_{uuid.uuid4().hex[:8]}"
        cls.conn = pyqql.connect(url="http://localhost:6333")
        cls.conn.cursor().execute(
            f"CREATE COLLECTION {cls.collection} (dense VECTOR(4, COSINE))"
        )
        cls.conn.cursor().execute(
            f"CREATE INDEX ON COLLECTION {cls.collection} FOR tag TYPE keyword"
        )
        cur = cls.conn.cursor()
        cur.executemany(
            f"UPSERT INTO {cls.collection} VALUES :rows",
            [
                {"id": 1, "vector": {"dense": [0.1, 0.2, 0.3, 0.4]}, "tag": "a"},
                {"id": 2, "vector": {"dense": [0.5, 0.6, 0.7, 0.8]}, "tag": "b"},
                {"id": 3, "vector": {"dense": [0.1, 0.2, 0.3, 0.5]}, "tag": "a"},
            ],
        )
        assert cur.rowcount == 3, cur.rowcount

    @classmethod
    def tearDownClass(cls):
        try:
            cls.conn.cursor().execute(f"DROP COLLECTION {cls.collection}")
        except Exception:
            pass
        cls.conn.close()

    def test_live_query_rows_and_description(self):
        cur = self.conn.cursor()
        cur.execute(f"QUERY [0.1, 0.2, 0.3, 0.4] FROM {self.collection} LIMIT 5")
        self.assertEqual([d[0] for d in cur.description], ["id", "score", "payload"])
        rows = cur.fetchall()
        self.assertEqual(len(rows), 3)
        self.assertEqual(rows[0][0], 1)
        self.assertIsInstance(rows[0][2], dict)
        self.assertEqual(rows[0][2]["tag"], "a")
        self.assertEqual(cur.rowcount, 3)
        columns = [d[0] for d in cur.description]
        as_dicts = [dict(zip(columns, row)) for row in rows]
        self.assertEqual(as_dicts[0]["id"], 1)

    def test_live_query_with_vector(self):
        cur = self.conn.cursor()
        cur.execute(
            f"QUERY [0.1, 0.2, 0.3, 0.4] FROM {self.collection} WITH VECTOR LIMIT 2"
        )
        self.assertEqual(
            [d[0] for d in cur.description], ["id", "score", "payload", "vector"]
        )
        self.assertIsNotNone(cur.fetchone()[3])

    def test_live_query_points_and_scroll(self):
        cur = self.conn.cursor()
        cur.execute(f"QUERY POINTS (1, 2) FROM {self.collection}")
        self.assertEqual([r[0] for r in cur.fetchall()], [1, 2])
        cur.execute(f"SCROLL FROM {self.collection} LIMIT 10")
        self.assertEqual(cur.rowcount, 3)
        ids = [row[0] for row in cur]
        self.assertEqual(sorted(ids), [1, 2, 3])

    def test_live_count_and_params(self):
        cur = self.conn.cursor()
        cur.execute(f"COUNT FROM {self.collection} WHERE tag = :t", params={"t": "a"})
        self.assertEqual(cur.fetchall(), [(2,)])

    def test_live_facet(self):
        cur = self.conn.cursor()
        cur.execute(f"FACET tag FROM {self.collection} LIMIT 10")
        rows = dict(cur.fetchall())
        self.assertEqual(rows, {"a": 2, "b": 1})

    def test_live_show_collections(self):
        cur = self.conn.cursor()
        cur.execute("SHOW COLLECTIONS")
        names = [row[0] for row in cur.fetchall()]
        self.assertIn(self.collection, names)

    def test_live_executemany_batch(self):
        cur = self.conn.cursor()
        cur.executemany(
            f"COUNT FROM {self.collection} WHERE tag = :t",
            [{"t": "a"}, {"t": "b"}],
        )
        self.assertEqual(cur.fetchall(), [(2,), (1,)])
        self.assertEqual(cur.rowcount, 2)

    def test_live_syntax_error_is_programming_error(self):
        cur = self.conn.cursor()
        with self.assertRaises(pyqql.ProgrammingError) as ctx:
            cur.execute("SELECT * FROM nowhere")
        self.assertIsNotNone(ctx.exception.code)

    def test_live_commit_noop_rollback_raises(self):
        self.assertIsNone(self.conn.commit())
        with self.assertRaises(pyqql.NotSupportedError):
            self.conn.rollback()


if __name__ == "__main__":
    unittest.main()
