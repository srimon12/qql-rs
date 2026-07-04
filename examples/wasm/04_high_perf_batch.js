// 04 High-Performance: Script parsing and batch parsing in the browser.
async function main() {
  const qql = await import('qql-wasm');

  const script = `
CREATE COLLECTION docs HYBRID;
INSERT INTO docs VALUES {id: 1, text: "first"};
INSERT INTO docs VALUES {id: 2, text: "second"};
QUERY "test" FROM docs LIMIT 10;
`;
  const stmts = qql.parse_all(script);
  console.log('=== Script Parsing (parse_all) ===');
  console.log(`Parsed ${stmts.length} statements from a .qql script:`);
  for (let i = 0; i < stmts.length; i++) {
    console.log(`  [${i}] ${stmts[i].substring(0, 80)}...`);
  }

  const queries = [
    "QUERY 'alpha' FROM docs LIMIT 5",
    "QUERY 'beta'  FROM docs LIMIT 5",
    "QUERY 'gamma' FROM docs LIMIT 5",
  ];
  const results = qql.parse_batch(queries);
  console.log('\n=== Batch Parsing (parse_batch) ===');
  console.log(`Parsed ${results.length} queries in a single WASM call:`);
  for (let i = 0; i < results.length; i++) {
    console.log(`  [${i}] ${results[i].substring(0, 80)}...`);
  }
}
main().catch(console.error);
