"""Medium to Expert Example: Custom HTTP Embedder, Multi-Tenant Security Gateway, and Plan Explanation."""
import pyqql

# 1. First-class custom embedding provider (Ollama / vLLM / OpenAI)
embedder = pyqql.HttpEmbedder(
    endpoint="http://localhost:11434/v1/embeddings",
    model="nomic-embed-text",
    dimension=768,
    api_key="optional-key"
)

client = pyqql.Client("http://localhost:6333", embedder=embedder)

# 2. Multi-tenant security gateway function
def build_secured_ast(user_role: str, tenant_id: str, query: str) -> dict:
    # Inject tenant isolation filter into query AST
    ast_stmt = pyqql.inject_filter(query, "tenant_id", "=", tenant_id)
    return ast_stmt.to_dict()

# 3. Query execution & plan explanation
raw_query = "QUERY 'acute myocardial infarction' FROM medical_records USING dense LIMIT 5"

secured_ast = build_secured_ast(user_role="viewer", tenant_id="hospital-east", query=raw_query)
print("=== Injected AST Filter ===")
print(secured_ast["Query"]["filter"])

print("\n=== Execution Plan ===")
plan = client.explain(raw_query)
print(plan)
