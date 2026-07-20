import time
import sys

# -----------------------------------------------------------------
# 1. Benchmark Import / Boot Time
# -----------------------------------------------------------------

print("Measuring Package Cold Start / Boot Time...")

# Measure qdrant-client import time
start_official = time.perf_counter()
import qdrant_client
from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct, VectorParams, Distance, Filter, FieldCondition, MatchValue
elapsed_official_import = time.perf_counter() - start_official
print(f"Official qdrant-client import: {elapsed_official_import * 1000:.2f} ms")

# Measure pyqql import time
start_qql = time.perf_counter()
import pyqql
elapsed_qql_import = time.perf_counter() - start_qql
print(f"pyqql import:                  {elapsed_qql_import * 1000:.2f} ms")


# -----------------------------------------------------------------
# 2. Initialize Clients
# -----------------------------------------------------------------

# HTTP REST Client for both (unifying transport interface)
official_client = QdrantClient(url="http://localhost:6333")
qql_client = pyqql.Client(url="http://localhost:6333", use_grpc=False)


# -----------------------------------------------------------------
# 3. Setup Collections
# -----------------------------------------------------------------

print("\nSetting up test collections...")
# Clean up if existing
if official_client.collection_exists("test_official"):
    official_client.delete_collection("test_official")
if official_client.collection_exists("test_qql"):
    official_client.delete_collection("test_qql")

# Recreate
official_client.create_collection(
    collection_name="test_official",
    vectors_config=VectorParams(size=4, distance=Distance.COSINE),
)

qql_client.execute('CREATE COLLECTION test_qql ( dense VECTOR ( 4, Cosine ) )')


# -----------------------------------------------------------------
# 4. Benchmark Inserts (1,000 points)
# -----------------------------------------------------------------

print("\nBenchmarking Insert Performance (1,000 points)...")

# Official SDK Upsert
official_points = [
    PointStruct(
        id=i,
        vector=[0.1, 0.2, 0.3, 0.4],
        payload={"category": "test", "value": i}
    )
    for i in range(1000)
]

start_official_insert = time.perf_counter()
official_client.upsert(
    collection_name="test_official",
    points=official_points,
    wait=True
)
elapsed_official_insert = time.perf_counter() - start_official_insert
print(f"Official SDK upsert:           {elapsed_official_insert * 1000:.2f} ms")

# PyQQL Batch Insert
# Construct batch insert query
values_list = []
for i in range(1000):
    values_list.append(f"{{id: {i}, vector: [0.1, 0.2, 0.3, 0.4], category: 'test', value: {i}}}")
qql_insert = f"INSERT INTO test_qql VALUES {', '.join(values_list)}"

start_qql_insert = time.perf_counter()
qql_client.execute(qql_insert)
elapsed_qql_insert = time.perf_counter() - start_qql_insert
print(f"pyqql batch insert:            {elapsed_qql_insert * 1000:.2f} ms")


# -----------------------------------------------------------------
# 5. Benchmark Search Queries (100 iterations)
# -----------------------------------------------------------------

print("\nBenchmarking Search Query Performance (100 iterations)...")

# Warmups
for _ in range(10):
    official_client.query_points(
        collection_name="test_official",
        query=[0.1, 0.2, 0.3, 0.4],
        query_filter=Filter(
            must=[FieldCondition(key="category", match=MatchValue(value="test"))]
        ),
        limit=10
    )
    qql_client.execute("QUERY [0.1, 0.2, 0.3, 0.4] FROM test_qql LIMIT 10 WHERE category = 'test'")

# Official SDK Search
start_official_search = time.perf_counter()
for _ in range(100):
    official_client.query_points(
        collection_name="test_official",
        query=[0.1, 0.2, 0.3, 0.4],
        query_filter=Filter(
            must=[FieldCondition(key="category", match=MatchValue(value="test"))]
        ),
        limit=10
    )
elapsed_official_search = time.perf_counter() - start_official_search
print(f"Official SDK search:           {elapsed_official_search * 1000:.2f} ms ({(elapsed_official_search / 100) * 1000:.2f} ms/op)")

# PyQQL Search
start_qql_search = time.perf_counter()
for _ in range(100):
    qql_client.execute("QUERY [0.1, 0.2, 0.3, 0.4] FROM test_qql LIMIT 10 WHERE category = 'test'")
elapsed_qql_search = time.perf_counter() - start_qql_search
print(f"pyqql search:                  {elapsed_qql_search * 1000:.2f} ms ({(elapsed_qql_search / 100) * 1000:.2f} ms/op)")


# Clean up collections
official_client.delete_collection("test_official")
official_client.delete_collection("test_qql")

# -----------------------------------------------------------------
# 6. Print Report
# -----------------------------------------------------------------

print("\n" + "=" * 55)
print(f"{'Metric':<25} | {'Official SDK':>12} | {'pyqql':>10}")
print("=" * 55)
print(f"{'Cold Start / Import':<25} | {elapsed_official_import * 1000:>9.1f} ms | {elapsed_qql_import * 1000:>7.1f} ms")
print(f"{'Insert (1,000 pts)':<25} | {elapsed_official_insert * 1000:>9.1f} ms | {elapsed_qql_insert * 1000:>7.1f} ms")
print(f"{'Search (100 runs)':<25} | {elapsed_official_search * 1000:>9.1f} ms | {elapsed_qql_search * 1000:>7.1f} ms")
print(f"{'Avg Search Latency':<25} | {(elapsed_official_search / 100) * 1000:>9.2f} ms | {(elapsed_qql_search / 100) * 1000:>7.2f} ms")
print("=" * 55)
