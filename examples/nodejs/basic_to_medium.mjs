import { Client, injectFilter } from '../../crates/nqql/index.js';

// 1. Initialize Client connected to Qdrant REST
const client = new Client({ url: "http://localhost:6333", useGrpc: false });

// 2. Inspect query execution plan
const plan = client.explain("QUERY 'machine learning' FROM papers LIMIT 5");
console.log("=== Query Execution Plan ===");
console.log(plan);

// 3. Inject tenant filter into AST
const rawQuery = "QUERY 'neural networks' FROM papers LIMIT 10";
const ast = injectFilter(rawQuery, "org_id", "=", '"acme-corp"');

console.log("\n=== Secured AST Object ===");
console.log(ast.Query.query_filter);
