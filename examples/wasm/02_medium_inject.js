// 02 Medium: Programmatic WHERE injection in the browser.
async function main() {
  const qql = await import('qql-wasm');

  const q = "QUERY 'machine learning transformer' FROM papers LIMIT 20";

  let r = qql.inject_filter(q, 'tenant_id', '=', '{"str": "acme-corp"}');
  console.log('=== String filter ===');
  console.log(r.substring(0, 400));

  r = qql.inject_filter(q, 'impact_factor', '>=', '{"float": 5.0}');
  console.log('\n=== Numeric filter ===');
  console.log(r.substring(0, 400));

  r = qql.inject_filter(q, 'is_published', '=', '{"bool": true}');
  console.log('\n=== Boolean filter ===');
  console.log(r.substring(0, 400));
}
main().catch(console.error);
