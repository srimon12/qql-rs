// 01 Basic: Parse, tokenize, and validate QQL queries in the browser.
async function main() {
  const qql = await import('qql-wasm');

  const ast = qql.parse('CREATE COLLECTION docs HYBRID WITH HNSW (m = 32)');
  console.log('=== Parsed AST ===');
  console.log(ast.substring(0, 500));

  console.log('\n=== Tokens ===');
  for (const t of qql.tokenize("QUERY 'vector database' FROM docs LIMIT 10")) {
    console.log(`  ${t.kind.padEnd(12)} ${t.text.padEnd(30)}  pos=${t.pos}`);
  }

  console.log('\n=== Validation ===');
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
