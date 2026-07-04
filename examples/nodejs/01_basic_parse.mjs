// Basic: Parse, tokenize, and validate QQL queries.
import { parse, tokenize, isValid } from 'nqql';

// Parse a CREATE COLLECTION statement
const ast = parse('CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)');
console.log('=== Parsed AST ===');
console.log(ast.substring(0, 500));
console.log();

// Tokenize a QUERY
const tokens = JSON.parse(tokenize("QUERY 'vector database' FROM docs LIMIT 10"));
console.log('=== Tokens ===');
for (const t of tokens) {
  console.log(`  ${t.kind.padEnd(12)} ${(t.text).padEnd(30)}  pos=${t.pos}`);
}
console.log();

// Validate queries
for (const q of [
  "QUERY 'hello' FROM docs LIMIT 5",
  'CREATE COLLECTION docs',
  'SELECT * FROM docs WHERE id = 1',
  '',
  'BOGUS STUFF',
]) {
  const valid = isValid(q);
  console.log(`  valid=${valid}  ${q}`);
}
