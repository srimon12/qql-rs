"""Offline tests for Client.scroll_cursor and scroll_cursor_async.

Network-free: a counting fake client serves fixed pages through the real
ExecutionReport class, so pagination, laziness, and validation are asserted
without a Qdrant server. Mirrors crates/nqql/test_scroll.js.
"""

import asyncio
import unittest

from pyqql import ExecutionReport, ScoredPoint
from pyqql import Client


def rows(ids):
    return [{"id": i, "score": 1.0, "payload": {"tag": "p-%s" % i}} for i in ids]


def scroll_report(data):
    return ExecutionReport.from_results(
        [{"operation": "SCROLL", "message": "ok", "hits": data}]
    )


class FakeClient:
    def __init__(self, pages):
        self.pages = list(pages)
        self.calls = []
        self.n = 0

    def execute(self, sql, params=None, on_error="stop"):
        self.calls.append({"sql": sql, "params": params})
        data = self.pages[self.n] if self.n < len(self.pages) else []
        self.n += 1
        return scroll_report(data)

    async def execute_async(self, sql, params=None, on_error="stop"):
        return self.execute(sql, params=params, on_error=on_error)


def collect_sync(gen):
    return list(gen)


def collect_async(gen):
    async def _collect():
        out = []
        async for point in gen:
            out.append(point)
        return out

    return asyncio.run(_collect())


def bind_scroll(client, *args, **kwargs):
    # Call the unbound Client method with a fake client instance.
    return Client.scroll_cursor.__get__(client)(*args, **kwargs)


class TestScrollCursor(unittest.TestCase):
    def test_lazy_pagination_stops_on_empty_page(self):
        client = FakeClient([rows([1, 2]), rows([3]), []])
        points = collect_sync(bind_scroll(client, "docs", batch_size=2))
        self.assertEqual([p.id for p in points], [1, 2, 3])
        self.assertEqual(len(client.calls), 3)
        self.assertNotIn("AFTER", client.calls[0]["sql"])
        self.assertIsNone(client.calls[0]["params"])
        self.assertIn("AFTER :cursor", client.calls[1]["sql"])
        self.assertEqual(client.calls[1]["params"], {"cursor": 2})
        self.assertEqual(client.calls[2]["params"], {"cursor": 3})
        self.assertIsInstance(points[0], ScoredPoint)
        self.assertEqual(points[0].get("tag"), "p-1")

    def test_empty_collection_stops_after_one_page(self):
        client = FakeClient([[]])
        self.assertEqual(collect_sync(bind_scroll(client, "docs")), [])
        self.assertEqual(len(client.calls), 1)

    def test_where_and_batch_size_shape_statement(self):
        client = FakeClient([[]])
        collect_sync(bind_scroll(client, "docs", batch_size=7, where="price < 150.0"))
        self.assertEqual(len(client.calls), 1)
        self.assertIn("WHERE price < 150.0", client.calls[0]["sql"])
        self.assertIn("LIMIT 7", client.calls[0]["sql"])
        self.assertLess(
            client.calls[0]["sql"].index("WHERE"), client.calls[0]["sql"].index("LIMIT")
        )

    def test_with_payload_strips_client_side(self):
        kept = collect_sync(bind_scroll(FakeClient([rows([1]), []]), "docs"))
        self.assertEqual(kept[0].payload, {"tag": "p-1"})
        stripped = collect_sync(
            bind_scroll(FakeClient([rows([1]), []]), "docs", with_payload=False)
        )
        self.assertIsNone(stripped[0].payload)
        self.assertEqual(stripped[0].id, 1)

    def test_with_vector_appends_clause_only_when_true(self):
        plain = FakeClient([[]])
        collect_sync(bind_scroll(plain, "docs"))
        self.assertNotIn("WITH VECTOR", plain.calls[0]["sql"])
        vectors = FakeClient([[]])
        collect_sync(bind_scroll(vectors, "docs", with_vector=True))
        self.assertIn("WITH VECTOR", vectors.calls[0]["sql"])

    def test_backpressure_single_page_buffered(self):
        client = FakeClient([rows([1, 2]), rows([3, 4]), []])
        it = bind_scroll(client, "docs", batch_size=2)
        self.assertEqual(next(it).id, 1)
        self.assertEqual(len(client.calls), 1)
        self.assertEqual(next(it).id, 2)
        self.assertEqual(len(client.calls), 1)
        self.assertEqual(next(it).id, 3)
        self.assertEqual(len(client.calls), 2)

    def test_option_validation_fails_closed(self):
        good = FakeClient([[]])
        with self.assertRaises(TypeError):
            collect_sync(bind_scroll(None, "docs"))
        with self.assertRaises(TypeError):
            collect_sync(bind_scroll(good, ""))
        with self.assertRaises((TypeError, ValueError)):
            collect_sync(bind_scroll(good, "docs", batch_size=0))
        with self.assertRaises(TypeError):
            collect_sync(bind_scroll(good, "docs", where=42))
        with self.assertRaises(TypeError):
            collect_sync(bind_scroll(good, "docs", with_payload="yes"))
        with self.assertRaises(TypeError):
            collect_sync(bind_scroll(good, "docs", params={"cursor": 1}))

    def test_escapes_hyphenated_collection(self):
        client = FakeClient([[]])
        collect_sync(bind_scroll(client, "my-hyphenated-collection"))
        self.assertTrue(client.calls[0]["sql"].startswith('SCROLL FROM "my-hyphenated-collection"'))

    def test_shard_key_routing(self):
        c1 = FakeClient([[]])
        collect_sync(bind_scroll(c1, "docs", shard_key="tenant-a"))
        self.assertIn("SHARD 'tenant-a'", c1.calls[0]["sql"])
        c2 = FakeClient([[]])
        collect_sync(bind_scroll(c2, "docs", shard_key=42))
        self.assertIn("SHARD 42", c2.calls[0]["sql"])
        c3 = FakeClient([[]])
        with self.assertRaises(TypeError):
            collect_sync(bind_scroll(c3, "docs", shard_key=True))

    def test_infinite_loop_guard(self):
        client = FakeClient([rows([1]), rows([1]), rows([1])])
        points = collect_sync(bind_scroll(client, "docs"))
        self.assertEqual(len(points), 1)
        self.assertEqual(len(client.calls), 2)

    def test_params_bind_across_pages(self):
        client = FakeClient([rows([1]), rows([2]), []])
        points = collect_sync(
            bind_scroll(
                client, "docs",
                where="price < :max_price",
                params={"max_price": 150},
                batch_size=1,
            )
        )
        self.assertEqual([p.id for p in points], [1, 2])
        self.assertEqual(client.calls[0]["params"], {"max_price": 150})
        self.assertEqual(client.calls[1]["params"], {"max_price": 150, "cursor": 1})

    def test_async_variant_pages(self):
        client = FakeClient([rows([1, 2]), rows([3]), []])

        async def _run():
            out = []
            gen = Client.scroll_cursor_async.__get__(client)("docs", batch_size=2)
            async for point in gen:
                out.append(point)
            return out

        points = asyncio.run(_run())
        self.assertEqual([p.id for p in points], [1, 2, 3])
        self.assertEqual(len(client.calls), 3)


if __name__ == "__main__":
    unittest.main()
