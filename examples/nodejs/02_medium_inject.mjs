// Medium: Programmatic WHERE injection — apply security filters to queries.
import { injectFilter, isValid } from 'nqql';

const userQuery = "QUERY 'machine learning transformer' FROM papers LIMIT 20";
console.log(`User query valid: ${isValid(userQuery)}`);

// Inject a tenant_id filter (string value)
const tenantQuery = injectFilter(
  userQuery, 'tenant_id', '=', '{"str": "acme-corp"}',
);
console.log('\n=== Tenant isolation ===');
console.log(tenantQuery.substring(0, 500));

// Inject a numeric threshold
const boosted = injectFilter(
  userQuery, 'impact_factor', '>=', '{"float": 5.0}',
);
console.log('\n=== Numeric threshold ===');
console.log(boosted.substring(0, 500));

// Inject a boolean flag
const published = injectFilter(
  userQuery, 'is_published', '=', '{"bool": true}',
);
console.log('\n=== Boolean filter ===');
console.log(published.substring(0, 500));
