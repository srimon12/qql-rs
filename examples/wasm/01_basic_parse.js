// Basic: Parse, tokenize, and validate QQL queries in the browser.
// Load with: import init, { parse, tokenize, is_valid } from 'qql-wasm';

async function main() {
  const qql = await import('qql-wasm');

  // Parse a CREATE COLLECTION statement
  const ast = qql.parse('CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)');
  console.log('=== Parsed AST ===');
  console.log(ast.substring(0, 500));

  // Tokenize a QUERY
  const tokens = qql.tokenize("QUERY 'vector database' FROM docs LIMIT 10");
  console.log('\n=== Tokens ===');
  for (const t of tokens) {
    console.log(`  ${t.kind.padEnd(12)} ${t.text.padEnd(30)}  pos=${t.pos}`);
  }

  // Validate queries
  for (const q of [
    "QUERY 'hello' FROM docs LIMIT 5",
    'CREATE COLLECTION docs',
    'SELECT * FROM docs WHERE id = 1',
    '',
    'BOGUS STUFF',
  ]) {
    console.log(`  valid=${qql.is_valid(q)}  ${q}`);
  }
}

main().catch(console.error);
