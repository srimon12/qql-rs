import json
import os
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


class TestWalSegmentMb(unittest.TestCase):
    """WAL segment capacity knob (MiB) exposed natively on local_executor."""

    def test_wal_segment_mb_persists_in_edge_config(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_wal_")
        try:
            client = pyqql_edge.local_executor(
                tmpdir, on_disk_payload=False, wal_segment_mb=1
            )
            self.assertTrue(client.execute("CREATE COLLECTION wal_docs").ok)
            client.close()

            config_path = os.path.join(tmpdir, "wal_docs", "edge_config.json")
            with open(config_path, encoding="utf-8") as fh:
                config = json.load(fh)
            self.assertEqual(
                config["wal_options"]["segment_capacity"],
                1 * 1024 * 1024,
                "1 MiB must lower to one MiB of bytes in the persisted config",
            )
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)

    def test_wal_segment_mb_rejects_invalid_values(self):
        tmpdir = tempfile.mkdtemp(prefix="pyqql_edge_wal_bad_")
        try:
            for bad in (0, -1, 1.5):
                with self.assertRaises(pyqql_edge.QqlValidationError) as ctx:
                    pyqql_edge.local_executor(tmpdir, wal_segment_mb=bad)
                self.assertEqual(
                    ctx.exception.code,
                    "QQL-VALIDATION-CONFIG",
                    f"wal_segment_mb={bad} must fail closed",
                )
                self.assertIsInstance(ctx.exception, ValueError)
        finally:
            shutil.rmtree(tmpdir, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
