import unittest
import pyqql


class TestDxImprovements(unittest.TestCase):
    def test_prepared_statement_binding_and_compile_route(self):
        # 1. execute(Stmt, params=...) / Stmt.bind prepared statements
        stmt = pyqql.parse("QUERY :v FROM test_coll LIMIT :lim")[0]
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

    def test_vector_truncation_for_readable_eyeball(self):
        # 7. bind() vector truncation for human readability
        vec = [0.1 * i for i in range(128)]
        s = pyqql.bind("QUERY :v FROM test_coll", {"v": vec}, truncate_vectors=True)
        self.assertIn("... (128 dims)", s)
        self.assertNotIn(str(vec[-1]), s)

    def test_dotted_and_nested_parameters(self):
        # 3. Dotted and nested parameter names
        nested_params = {"loc": {"lat": 12.34, "lon": 56.78}}
        s = pyqql.bind(
            "QUERY [0.1, 0.2] FROM test_coll WHERE lat = :loc.lat AND lon = :loc.lon",
            nested_params,
        )
        self.assertEqual(
            s,
            "QUERY [0.1, 0.2] FROM test_coll WHERE lat = 12.34 AND lon = 56.78",
        )

        flat_params = {"loc.lat": 12.34, "loc.lon": 56.78}
        s2 = pyqql.bind(
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
                    "data": {"result": {"count": 42}},
                },
            ],
            "succeeded": 3,
            "failed": 0,
        }

        rep = pyqql.ExecutionReport(rep_dict)
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
        self.assertIsInstance(hits[0].id, int)
        self.assertIsInstance(hits[1].id, str)
        self.assertEqual(hits[0].score, 0.95)
        self.assertEqual(hits[0]["title"], "Doc 1")
        self.assertEqual(hits[0].get("title"), "Doc 1")
        self.assertEqual(hits[0].collection, "coll_a")

        # 4. Facet normalized hits
        facet = rep.facet(1)
        self.assertEqual(len(facet), 2)
        self.assertEqual(facet[0]["value"], "tech")

        # Count accessor
        self.assertEqual(rep.count(2), 42)


if __name__ == "__main__":
    unittest.main()
