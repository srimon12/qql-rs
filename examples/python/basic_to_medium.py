"""Basic to Medium Example: QQL Client Connection, Query Execution, and Filter Injection."""
import pyqql

# 1. Initialize Client connected to Qdrant REST
client = pyqql.Client("http://localhost:6333", use_grpc=False)

# 2. Inspect query execution plan
plan = client.explain("QUERY 'cardiology treatment' FROM medical_records LIMIT 5")
print("=== Query Execution Plan ===")
print(plan)

# 3. Inject tenant security filter into AST
raw_query = "QUERY 'patient records' FROM medical_records LIMIT 10"
ast_stmt = pyqql.inject_filter(raw_query, "tenant_id", "=", "acme-corp")
ast_dict = ast_stmt.to_dict()

print("\n=== Injected AST Filter ===")
print(ast_dict["Query"]["filter"])
