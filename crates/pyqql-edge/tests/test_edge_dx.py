import shutil
import tempfile
import unittest
import pyqql_edge


class TestLiveEdgeDx(unittest.TestCase):
    def test_live_edge_execution_with_prepared_stmt_and_scoped_params(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_dx_")
        try:
            client = pyqql_edge.local_executor(tmpdir, on_disk_payload=False)
            res1 = client.execute("CREATE COLLECTION test_dx")
            self.assertTrue(res1.ok)

            # 1. Execute with Stmt object + params
            count_stmt = pyqql_edge.parse("COUNT FROM test_dx")[0]
            rep = client.execute(count_stmt)
            self.assertTrue(rep.ok)
            self.assertEqual(rep.count(0), 0)

            # 2. Scoped batch params
            batch_stmts = [
                "COUNT FROM test_dx",
                "COUNT FROM test_dx",
            ]
            batch_rep = client.execute(batch_stmts)
            self.assertTrue(batch_rep.ok)
            self.assertEqual(len(batch_rep.results), 2)
            self.assertEqual(batch_rep.count(0), 0)
            self.assertEqual(batch_rep.count(1), 0)

            client.close()
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)


class TestCloseContract(unittest.TestCase):
    """N5: the edge client's close gate is typed, not a bare RuntimeError."""

    def test_close_raises_typed_client_closed(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_close_")
        try:
            client = pyqql_edge.local_executor(tmpdir, on_disk_payload=False)
            client.close()
            self.assertTrue(client.is_closed)
            with self.assertRaises(pyqql_edge.QqlExecutionError) as ctx:
                client.execute("QUERY 'x' FROM docs")
            self.assertEqual(ctx.exception.code, "QQL-CLIENT-CLOSED")
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
