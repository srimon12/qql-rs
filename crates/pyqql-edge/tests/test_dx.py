import importlib
import os
import unittest

SDK_NAME = os.environ.get(
    "PYQQL_MODULE",
    "pyqql_edge" if "pyqql-edge" in os.path.abspath(__file__) else "pyqql",
)
sdk = importlib.import_module(SDK_NAME)


class TestDxImprovements(unittest.TestCase):
    def test_prepared_statement_binding_and_compile_route(self):
        # 1. execute(Stmt, params=...) / Stmt.bind prepared statements
        stmt = sdk.parse("QUERY :v FROM test_coll LIMIT :lim")[0]
        self.assertIn(":v", repr(stmt))
        self.assertEqual(str(stmt), "QUERY :v FROM test_coll LIMIT :lim")

        bound = stmt.bind({"v": [0.1, 0.2, 0.3], "lim": 5})
        self.assertEqual(
            str(bound), "QUERY VECTOR [0.1, 0.2, 0.3] FROM test_coll LIMIT 5"
        )

        route = stmt.compile_route(params={"v": [0.1, 0.2, 0.3], "lim": 5})
        self.assertEqual(route["method"], "POST")
        self.assertEqual(route["path"], "/collections/test_coll/points/query")
        self.assertEqual(route["payload"]["limit"], 5)
        self.assertEqual(len(route["payload"]["query"]["nearest"]), 3)

        # Module compile_query and Client.compile accept params too
        # (parity with nqql compileQuery / Client.compile).
        route2 = sdk.compile_query(
            "QUERY :v FROM test_coll LIMIT :lim",
            params={"v": [0.1, 0.2, 0.3], "lim": 5},
        )
        self.assertEqual(route2["payload"]["limit"], 5)
        self.assertEqual(len(route2["payload"]["query"]["nearest"]), 3)

    def test_vector_truncation_for_readable_eyeball(self):
        # 7. bind() vector truncation for human readability
        vec = [0.1 * i for i in range(128)]
        s = sdk.bind("QUERY :v FROM test_coll", {"v": vec}, truncate_vectors=True)
        self.assertIn("... (128 dims)", s)
        self.assertNotIn(str(vec[-1]), s)

    def test_dotted_and_nested_parameters(self):
        # 3. Dotted and nested parameter names
        nested_params = {"loc": {"lat": 12.34, "lon": 56.78}}
        s = sdk.bind(
            "QUERY [0.1, 0.2] FROM test_coll WHERE lat = :loc.lat AND lon = :loc.lon",
            nested_params,
        )
        self.assertEqual(
            s,
            "QUERY [0.1, 0.2] FROM test_coll WHERE lat = 12.34 AND lon = 56.78",
        )

        flat_params = {"loc.lat": 12.34, "loc.lon": 56.78}
        s2 = sdk.bind(
            "QUERY [0.1, 0.2] FROM test_coll WHERE lat = :loc.lat AND lon = :loc.lon",
            flat_params,
        )
        self.assertEqual(
            s2,
            "QUERY [0.1, 0.2] FROM test_coll WHERE lat = 12.34 AND lon = 56.78",
        )

    def test_execution_report_and_scored_point(self):
        # 4, 5, 8. ScoredPoint, ExecutionReport backward compatibility and typed accessors
        rep_dict = {
            "ok": True,
            "results": [
                {
                    "ok": True,
                    "operation": "QUERY",
                    "message": "Found 2 hits",
                    "data": [
                        {
                            "id": 936746218411023069,
                            "score": 0.95,
                            "payload": {"title": "Doc 1"},
                            "text": "Doc 1",
                            "collection": "coll_a",
                        },
                        {
                            "id": "c87bb3c1-a201-447a-8f5f-1555df27d14d",
                            "score": 0.82,
                            "payload": {"title": "Doc 2"},
                            "text": "Doc 2",
                            "collection": "coll_a",
                        },
                    ],
                },
                {
                    "ok": True,
                    "operation": "FACET",
                    "message": "Found 2 facet hit(s)",
                    "data": [
                        {"value": "tech", "count": 10},
                        {"value": "news", "count": 4},
                    ],
                },
                {
                    "ok": True,
                    "operation": "COUNT",
                    "message": "Count: 42",
                    "data": {"count": 42},
                },
            ],
            "succeeded": 3,
            "failed": 0,
        }

        rep = sdk.ExecutionReport(rep_dict)
        # Backward compatibility
        self.assertTrue(rep.ok)
        self.assertTrue(rep["ok"])
        self.assertEqual(rep.succeeded, 3)
        self.assertEqual(rep["succeeded"], 3)

        # 8. Typed ScoredPoint
        hits = rep.hits(0)
        self.assertEqual(len(hits), 2)
        # 5. ID integer vs string
        self.assertEqual(hits[0].id, 936746218411023069)
        self.assertEqual(hits[0]["id"], 936746218411023069)
        self.assertIsInstance(hits[0].id, int)
        self.assertIsInstance(hits[1].id, str)
        self.assertEqual(hits[0].score, 0.95)
        self.assertEqual(hits[0]["score"], 0.95)
        self.assertEqual(hits[0]["title"], "Doc 1")
        self.assertEqual(hits[0].get("title"), "Doc 1")
        self.assertEqual(hits[0].collection, "coll_a")

        # Point IDs accessor
        self.assertEqual(
            rep.ids(0),
            [936746218411023069, "c87bb3c1-a201-447a-8f5f-1555df27d14d"],
        )

        # 4. Facet normalized hits
        facet = rep.facet(1)
        self.assertEqual(len(facet), 2)
        self.assertEqual(facet[0]["value"], "tech")

        # Count accessor
        self.assertEqual(rep.count(2), 42)

        # Vector default and negative index safety
        self.assertIsNone(hits[0].vector)
        self.assertEqual(rep.hits(-10), [])
        self.assertEqual(rep.points(-10), [])
        self.assertEqual(rep.facet(-10), [])
        self.assertEqual(rep.count(-10), 0)
        self.assertEqual(rep.groups(-10), [])

    def test_execution_report_groups_accessor(self):
        # GROUP BY results normalize through report.groups() (pyqql parity
        # with nqql's ExecutionReport.groups()).
        ExecutionReport = sdk.ExecutionReport

        nested = ExecutionReport(
            {
                "ok": True,
                "succeeded": 1,
                "failed": 0,
                "results": [
                    {
                        "ok": True,
                        "operation": "QUERY_GROUPS",
                        "message": "Found 2 group(s)",
                        "data": {
                            "result": {
                                "groups": [
                                    {"id": "a", "hits": [{"id": 1, "score": 0.9}]},
                                    {"id": "b", "hits": [{"id": 2, "score": 0.8}]},
                                ]
                            },
                            "status": "ok",
                        },
                    }
                ],
            }
        )
        self.assertEqual(len(nested.groups()), 2)
        self.assertEqual(nested.groups()[0]["id"], "a")
        bare = ExecutionReport(
            {
                "ok": True,
                "succeeded": 1,
                "failed": 0,
                "results": [
                    {
                        "ok": True,
                        "operation": "QUERY_GROUPS",
                        "message": "Found 1 group(s)",
                        "data": {"groups": [{"id": "x", "hits": []}]},
                    }
                ],
            }
        )
        self.assertEqual(bare.groups()[0]["id"], "x")
        self.assertEqual(ExecutionReport().groups(), [])

    def test_typed_buffer_vector_params(self):
        # Buffer-protocol vectors bind identically to plain lists (no server).
        import array

        vec = [0.1, 0.2, 0.3, 0.4]
        q = "QUERY :v FROM test_coll USING dense LIMIT 2"
        expected = str(sdk.parse(q)[0].bind({"v": vec}))
        self.assertIn("[0.1, 0.2, 0.3, 0.4]", expected)

        for buf in (
            array.array("d", vec),
            array.array("f", vec),
            memoryview(array.array("d", vec)),
        ):
            with self.subTest(buf=type(buf).__name__):
                self.assertEqual(str(sdk.parse(q)[0].bind({"v": buf})), expected)

        np = None
        try:
            import numpy

            np = numpy
        except ImportError:
            pass
        if np is not None:
            for arr in (
                np.array(vec, dtype=np.float64),
                np.array(vec, dtype=np.float32),
            ):
                with self.subTest(buf=f"numpy-{arr.dtype}"):
                    self.assertEqual(str(sdk.parse(q)[0].bind({"v": arr})), expected)
            # Non-contiguous views keep the tolist() path (same values).
            strided = np.array(vec * 2, dtype=np.float64)[::2]
            strided_expected = str(sdk.parse(q)[0].bind({"v": list(strided)}))
            self.assertEqual(
                str(sdk.parse(q)[0].bind({"v": strided})), strided_expected
            )
            # 2-D arrays keep prior behavior: tolist() path, binds exactly
            # like the equivalent nested lists.
            two_d = np.array([[0.1, 0.2], [0.3, 0.4]])
            self.assertEqual(
                str(sdk.parse(q)[0].bind({"v": two_d})),
                str(sdk.parse(q)[0].bind({"v": [[0.1, 0.2], [0.3, 0.4]]})),
            )

        # Module bind + compile_query equivalence, incl. flat {data, dim}.
        self.assertEqual(
            sdk.bind(q, {"v": array.array("d", vec)}), sdk.bind(q, {"v": vec})
        )
        self.assertEqual(
            sdk.compile_query(q, {"v": array.array("d", vec)}),
            sdk.compile_query(q, {"v": vec}),
        )
        flat = sdk.bind(
            "QUERY VECTOR :m FROM test_coll USING dense",
            {"m": {"data": vec, "dim": 2}},
        )
        nested = sdk.bind(
            "QUERY VECTOR :m FROM test_coll USING dense",
            {"m": [[0.1, 0.2], [0.3, 0.4]]},
        )
        # String bind renders the dict literally; both re-parse to the same
        # chunked MultiDense statement.
        self.assertEqual(
            str(sdk.parse(flat)[0]), str(sdk.parse(nested)[0])
        )

        # Positional ? with buffers.
        qp = "QUERY ? FROM test_coll USING dense LIMIT 1"
        self.assertEqual(
            str(sdk.parse(qp)[0].bind([array.array("d", vec)])),
            str(sdk.parse(qp)[0].bind([vec])),
        )

        # Bytes are not float buffers — same unsupported-value error as before.
        with self.assertRaises(ValueError):
            sdk.parse(q)[0].bind({"v": b"\x00\x01"})

    def test_long_float_lists_pack_as_f32_vectors(self):
        # Flat list[float] params with >= 32 elements bind as f32 vectors
        # (same representation as numpy / array.array buffers); shorter and
        # nested float lists plus int lists keep exact Value::List semantics.
        q = "QUERY :v FROM test_coll USING dense LIMIT 2"
        f64_sentinel = 1.0000000001

        long_bound = str(sdk.parse(q)[0].bind({"v": [f64_sentinel] + [0.5] * 31}))
        self.assertIn("[1.0, 0.5", long_bound)
        self.assertNotIn(str(f64_sentinel), long_bound)

        short_bound = str(
            sdk.parse("UPSERT INTO c VALUES {id: 1, x: :v}")[0].bind(
                {"v": [f64_sentinel] + [0.5] * 30}
            )
        )
        self.assertIn("[1.0000000001, 0.5", short_bound)

        nested_bound = sdk.bind(
            "QUERY VECTOR :v FROM test_coll USING dense",
            {"v": [[f64_sentinel, 2.0]]},
        )
        self.assertIn("[[1.0000000001, 2.0]]", nested_bound)

        int_bound = sdk.bind(
            "QUERY [0.1] FROM test_coll WHERE x = :v", {"v": list(range(32))}
        )
        self.assertIn("[0, 1, 2,", int_bound)

        # Non-finite floats still fail closed on both sides of the threshold.
        for bad_vec in ([float("nan")] + [0.0] * 31, [float("nan")]):
            with self.subTest(length=len(bad_vec)):
                with self.assertRaises(ValueError):
                    sdk.bind(q, {"v": bad_vec})

        # Equivalence: list-bound long vectors match numpy f32 buffers.
        try:
            import numpy
        except ImportError:
            return
        vec = [0.1 * i for i in range(128)]
        self.assertEqual(
            str(sdk.parse(q)[0].bind({"v": vec})),
            str(sdk.parse(q)[0].bind({"v": numpy.array(vec, dtype=numpy.float32)})),
        )

    def test_upsert_many_surface(self):
        # Bulk ingest lives on the client next to execute — one `:rows`
        # template prepared once, no hand-rolled batch loops. Offline:
        # surface parity only (live chunking is covered by Rust mock
        # tests + the vs-qdrant harness, no Qdrant server in CI).
        self.assertTrue(callable(sdk.Client.upsert_many))


if __name__ == "__main__":
    unittest.main()
