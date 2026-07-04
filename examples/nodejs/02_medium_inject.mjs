// 02 Medium: Programmatic WHERE injection.
import { injectFilter } from 'nqql';

const q = "QUERY 'machine learning transformer' FROM papers LIMIT 20";

let r = injectFilter(q, 'tenant_id', '=', '{"str": "acme-corp"}');
console.log('=== String filter ===');
console.log(r.substring(0, 400));

r = injectFilter(q, 'impact_factor', '>=', '{"float": 5.0}');
console.log('\n=== Numeric filter ===');
console.log(r.substring(0, 400));

r = injectFilter(q, 'is_published', '=', '{"bool": true}');
console.log('\n=== Boolean filter ===');
console.log(r.substring(0, 400));
