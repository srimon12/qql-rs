// Medium: Programmatic WHERE injection — apply security filters to queries.
async function main() {
  const qql = await import('qql-wasm');

  const userQuery = "QUERY 'machine learning transformer' FROM papers LIMIT 20";
  console.log(`User query valid: ${qql.is_valid(userQuery)}`);

  // Inject a tenant_id filter (string value)
  const tenantQuery = qql.inject_filter(
    userQuery, 'tenant_id', '=', '{"str": "acme-corp"}',
  );
  console.log('\n=== Tenant isolation ===');
  console.log(tenantQuery.substring(0, 500));

  // Inject a numeric threshold
  const boosted = qql.inject_filter(
    userQuery, 'impact_factor', '>=', '{"float": 5.0}',
  );
  console.log('\n=== Numeric threshold ===');
  console.log(boosted.substring(0, 500));

  // Inject a boolean flag
  const published = qql.inject_filter(
    userQuery, 'is_published', '=', '{"bool": true}',
  );
  console.log('\n=== Boolean filter ===');
  console.log(published.substring(0, 500));
}

main().catch(console.error);
