use serde_json::Value;

/// Shared helper: convert a `using` field value to its QQL representation.
fn using_str(using: &str) -> String {
    match using.to_lowercase().as_str() {
        "hybrid" => "USING HYBRID".to_string(),
        "sparse" => "USING SPARSE".to_string(),
        _ => format!("USING '{}'", escape_qql_string(using)),
    }
}

/// Shared helper: convert a `lookup_from` object to its QQL representation.
fn lookup_from_str(lookup: &serde_json::Map<String, Value>) -> Option<String> {
    let coll = lookup.get("collection").and_then(|v| v.as_str())?;
    let vec_name = lookup.get("vector").and_then(|v| v.as_str());
    Some(match vec_name {
        Some(vn) => format!("LOOKUP FROM {} VECTOR '{}'", coll, escape_qql_string(vn)),
        None => format!("LOOKUP FROM {}", coll),
    })
}

fn sanitize_collection_name(name: &str) -> String {
    if name.is_empty() {
        return "unknown".to_string();
    }
    let out: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

fn extract_collection(path: &str) -> String {
    let path = path.trim_start_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() >= 2 && parts[0] == "collections" {
        sanitize_collection_name(parts[1])
    } else {
        "unknown".to_string()
    }
}

fn format_id(id: &Value) -> String {
    match id {
        Value::String(s) => format!("'{}'", s.replace('\'', "\\'")),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 {
                    format!("{}", f as i64)
                } else {
                    format!("{}", f)
                }
            } else {
                format!("'{}'", n)
            }
        }
        _ => format!("'{}'", id),
    }
}

fn format_id_list(ids: &[Value]) -> String {
    ids.iter().map(format_id).collect::<Vec<_>>().join(", ")
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.is_finite() {
                    format!("{}", f as i64)
                } else {
                    format!("{}", f)
                }
            } else {
                format!("{}", n)
            }
        }
        Value::String(s) => format!("'{}'", s.replace('\'', "\\'")),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(map) => format_map(map),
    }
}

fn format_map(map: &serde_json::Map<String, Value>) -> String {
    let mut parts = Vec::new();
    for (k, v) in map {
        parts.push(format!("'{}': {}", k, format_value(v)));
    }
    format!("{{{}}}", parts.join(", "))
}

fn format_vector(vec: &Value) -> Option<String> {
    match vec {
        Value::Array(arr) => {
            let items: Vec<String> = arr
                .iter()
                .map(|v| match v {
                    Value::Number(n) => n.to_string(),
                    _ => v.to_string(),
                })
                .collect();
            if items.is_empty() {
                None
            } else {
                Some(format!("[{}]", items.join(", ")))
            }
        }
        _ => None,
    }
}

fn build_payload_dict(payload: &serde_json::Map<String, Value>) -> String {
    if payload.is_empty() {
        return "{}".to_string();
    }
    let mut parts = Vec::new();
    for (k, v) in payload {
        parts.push(format!("'{}': {}", k, format_value(v)));
    }
    format!("{{{}}}", parts.join(", "))
}

fn escape_qql_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn convert_filter_to_qql(filter: &Value) -> Result<String, String> {
    if let Some(obj) = filter.as_object() {
        let mut parts = Vec::new();

        if let Some(must) = obj.get("must").and_then(|v| v.as_array()) {
            for cond in must {
                let s = convert_condition(cond)?;
                if !s.is_empty() {
                    parts.push(s);
                }
            }
        }

        if let Some(should) = obj.get("should").and_then(|v| v.as_array()) {
            if should.len() == 1 {
                let s = convert_condition(&should[0])?;
                if !s.is_empty() {
                    parts.push(s);
                }
            } else if should.len() > 1 {
                let inner: Vec<String> = should
                    .iter()
                    .filter_map(|c| convert_condition(c).ok())
                    .filter(|s| !s.is_empty())
                    .collect();
                if inner.len() == 1 {
                    parts.push(inner.into_iter().next().unwrap());
                } else if !inner.is_empty() {
                    parts.push(format!("({})", inner.join(" OR ")));
                }
            }
        }

        if let Some(must_not) = obj.get("must_not").and_then(|v| v.as_array()) {
            for cond in must_not {
                let s = convert_condition(cond)?;
                if !s.is_empty() {
                    parts.push(format!("NOT ({})", s));
                }
            }
        }

        if parts.is_empty() {
            return Ok(String::new());
        }
        if parts.len() == 1 {
            Ok(parts.into_iter().next().unwrap())
        } else {
            Ok(format!("({})", parts.join(" AND ")))
        }
    } else {
        Ok(String::new())
    }
}

fn convert_condition(cond: &Value) -> Result<String, String> {
    let obj = match cond.as_object() {
        Some(o) => o,
        None => return Ok(String::new()),
    };

    if let Some(has_id) = obj.get("has_id").and_then(|v| v.as_array()) {
        let ids: Vec<String> = has_id.iter().map(format_id).collect();
        return Ok(format!("id IN ({})", ids.join(", ")));
    }

    if let Some(is_empty) = obj.get("is_empty").and_then(|v| v.as_object()) {
        if let Some(key) = is_empty.get("key").and_then(|v| v.as_str()) {
            return Ok(format!("{} IS EMPTY", key));
        }
    }

    if let Some(is_null) = obj.get("is_null").and_then(|v| v.as_object()) {
        if let Some(key) = is_null.get("key").and_then(|v| v.as_str()) {
            return Ok(format!("{} IS NULL", key));
        }
    }

    let key = obj.get("key").and_then(|v| v.as_str()).unwrap_or("");

    if let Some(match_obj) = obj.get("match").and_then(|v| v.as_object()) {
        if let Some(value) = match_obj.get("value") {
            return Ok(format!("{} = {}", key, format_value(value)));
        }
        if let Some(keyword) = match_obj.get("keyword") {
            return Ok(format!("{} = {}", key, format_value(keyword)));
        }
        if let Some(integer) = match_obj.get("integer") {
            return Ok(format!("{} = {}", key, format_value(integer)));
        }
        if let Some(boolean) = match_obj.get("boolean") {
            return Ok(format!("{} = {}", key, format_value(boolean)));
        }
        if let Some(text) = match_obj.get("text").and_then(|v| v.as_str()) {
            return Ok(format!("{} MATCH '{}'", key, escape_qql_string(text)));
        }
        if let Some(text_any) = match_obj.get("text_any").and_then(|v| v.as_str()) {
            return Ok(format!(
                "{} MATCH ANY '{}'",
                key,
                escape_qql_string(text_any)
            ));
        }
        if let Some(any) = match_obj.get("any").and_then(|v| v.as_array()) {
            let vals: Vec<String> = any.iter().map(format_value).collect();
            return Ok(format!("{} IN ({})", key, vals.join(", ")));
        }
        if let Some(except) = match_obj.get("except").and_then(|v| v.as_array()) {
            let vals: Vec<String> = except.iter().map(format_value).collect();
            return Ok(format!("{} NOT IN ({})", key, vals.join(", ")));
        }
    }

    if let Some(range) = obj.get("range").and_then(|v| v.as_object()) {
        let gte = range.get("gte");
        let gt = range.get("gt");
        let lte = range.get("lte");
        let lt = range.get("lt");

        if (gte.is_some() || gt.is_some()) && (lte.is_some() || lt.is_some()) {
            let low = gte.or(gt).unwrap();
            let high = lte.or(lt).unwrap();
            return Ok(format!(
                "{} BETWEEN {} AND {}",
                key,
                format_value(low),
                format_value(high)
            ));
        }
        if let Some(v) = gte {
            return Ok(format!("{} >= {}", key, format_value(v)));
        }
        if let Some(v) = gt {
            return Ok(format!("{} > {}", key, format_value(v)));
        }
        if let Some(v) = lte {
            return Ok(format!("{} <= {}", key, format_value(v)));
        }
        if let Some(v) = lt {
            return Ok(format!("{} < {}", key, format_value(v)));
        }
    }

    if let Some(_geo_box) = obj.get("geo_bounding_box").and_then(|v| v.as_object()) {
        return Ok(format!("{} GEO_BBOX (...)", key));
    }

    if let Some(_geo_radius) = obj.get("geo_radius").and_then(|v| v.as_object()) {
        return Ok(format!("{} GEO_RADIUS (...)", key));
    }

    Ok(String::new())
}

pub fn json_to_qql(input: &str) -> Result<Vec<String>, String> {
    json_to_qql_with_collection(input, "unknown")
}

pub fn json_to_qql_with_collection(input: &str, collection: &str) -> Result<Vec<String>, String> {
    let input = input.trim();
    let collection = if collection.is_empty() {
        "unknown"
    } else {
        collection
    };
    let collection = sanitize_collection_name(collection);

    let raw: Value = serde_json::from_str(input).map_err(|e| format!("invalid JSON: {}", e))?;

    // Check for wrapped request with method + path
    if let Some(obj) = raw.as_object() {
        let method = obj.get("method").and_then(|v| v.as_str());
        let path = obj.get("path").and_then(|v| v.as_str());
        if let (Some(method), Some(path)) = (method, path) {
            let body = obj.get("body").or_else(|| obj.get("request")).cloned();
            return convert_by_endpoint(method, path, body.as_ref());
        }
    }

    convert_by_structure(&raw, &collection)
}

fn convert_by_endpoint(
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Vec<String>, String> {
    let path = path.trim_start_matches('/');
    let collection = extract_collection(path);

    match (method, path) {
        // PUT /collections/{name}
        (m, _)
            if m == "PUT"
                && path.starts_with("collections/")
                && !path.contains("/points")
                && !path.contains("/index") =>
        {
            convert_create_collection(body.unwrap_or(&Value::Null), &collection)
        }
        // DELETE /collections/{name}
        (m, _)
            if m == "DELETE" && path.starts_with("collections/") && !path.contains("/points") =>
        {
            Ok(vec![format!("DROP COLLECTION {}", collection)])
        }
        // PUT /collections/{name}/points
        (m, _) if m == "PUT" && path.ends_with("/points") => {
            convert_upsert(body.unwrap_or(&Value::Null), &collection)
        }
        // POST /collections/{name}/points/query
        (m, _) if m == "POST" && path.ends_with("/points/query") => {
            if let Some(b) = body {
                convert_formula_query(b, &collection)
            } else {
                Ok(vec![])
            }
        }
        // POST /collections/{name}/points/search
        (m, _) if m == "POST" && path.ends_with("/points/search") => {
            convert_search(body.unwrap_or(&Value::Null), &collection)
        }
        // POST /collections/{name}/points/recommend
        (m, _) if m == "POST" && path.ends_with("/points/recommend") => {
            convert_recommend(body.unwrap_or(&Value::Null), &collection)
        }
        // POST /collections/{name}/points/discover
        (m, _) if m == "POST" && path.ends_with("/points/discover") => {
            convert_discover(body.unwrap_or(&Value::Null), &collection)
        }
        // POST /collections/{name}/points/scroll
        (m, _) if m == "POST" && path.ends_with("/points/scroll") => {
            convert_scroll(body.unwrap_or(&Value::Null), &collection)
        }
        // POST /collections/{name}/points (get points)
        (m, _)
            if m == "POST"
                && path.ends_with("/points")
                && !path.contains("/search")
                && !path.contains("/recommend") =>
        {
            convert_get_points(body.unwrap_or(&Value::Null), &collection)
        }
        // POST /collections/{name}/points/delete
        (m, _) if m == "POST" && path.ends_with("/points/delete") => {
            convert_delete_points(body.unwrap_or(&Value::Null), &collection)
        }
        // POST /collections/{name}/points/payload
        (m, _) if m == "POST" && path.ends_with("/points/payload") => {
            convert_set_payload(body.unwrap_or(&Value::Null), &collection)
        }
        // PUT /collections/{name}/index
        (m, _) if m == "PUT" && path.ends_with("/index") => {
            convert_create_index(body.unwrap_or(&Value::Null), &collection)
        }
        _ => Err(format!("unsupported endpoint: {} {}", method, path)),
    }
}

fn convert_by_structure(raw: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return Err("expected a JSON object".to_string()),
    };

    has_prefetch_and_query(obj, collection)
        .or_else(|| has_top_level_prefetch(obj, collection))
        .or_else(|| has_batch_search(obj))
        .or_else(|| has_set_payload(obj, collection))
        .or_else(|| has_points_field(obj, collection))
        .or_else(|| has_vector_field(obj, collection))
        .or_else(|| has_positive_field(obj, collection))
        .or_else(|| has_target_field(obj, collection))
        .or_else(|| has_ids_field(obj, collection))
        .or_else(|| has_vectors_config(obj, collection))
        .or_else(|| has_field_name(obj, collection))
        .or_else(|| has_filter_field(obj, collection))
        .unwrap_or_else(|| Err("cannot detect operation from JSON structure".to_string()))
}

fn convert_upsert(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid upsert JSON".to_string()),
    };
    let points = match obj.get("points").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Err("no points in upsert payload".to_string()),
    };

    let mut stmts = Vec::new();
    for point in points {
        let pobj = match point.as_object() {
            Some(o) => o,
            None => continue,
        };

        let mut payload = serde_json::Map::new();
        if let Some(p) = pobj.get("payload").and_then(|v| v.as_object()) {
            for (k, v) in p {
                payload.insert(k.clone(), v.clone());
            }
        }
        if let Some(id) = pobj.get("id") {
            payload.insert("id".to_string(), id.clone());
        }
        if let Some(vec) = pobj.get("vector") {
            payload.insert("vector".to_string(), vec.clone());
        } else if let Some(vectors) = pobj.get("vectors").and_then(|v| v.as_object()) {
            let mut vmap = serde_json::Map::new();
            for (k, v) in vectors {
                vmap.insert(k.clone(), v.clone());
            }
            payload.insert("vector".to_string(), Value::Object(vmap));
        }

        let values = build_payload_dict(&payload);
        stmts.push(format!("INSERT INTO {} VALUES {}", collection, values));
    }

    if stmts.is_empty() {
        return Err("no points found in upsert payload".to_string());
    }
    Ok(stmts)
}

fn convert_search(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid search JSON".to_string()),
    };

    let mut parts = Vec::new();
    let mut qql_args = Vec::new();

    // Determine query text/vector/ID
    let query_raw = obj.get("query");
    let vector = obj.get("vector");

    if let Some(q) = query_raw {
        if let Some(s) = q.as_str() {
            qql_args.push(format!("QUERY '{}'", escape_qql_string(s)));
        } else if let Some(_arr) = q.as_array() {
            let vec_str = format_vector(q).unwrap_or_default();
            qql_args.push(format!("QUERY {}", vec_str));
        } else if let Some(qobj) = q.as_object() {
            if let Some(text) = qobj.get("text").and_then(|v| v.as_str()) {
                if let Some(model) = qobj.get("model").and_then(|v| v.as_str()) {
                    qql_args.push(format!(
                        "QUERY '{}' USING MODEL '{}'",
                        escape_qql_string(text),
                        escape_qql_string(model)
                    ));
                } else {
                    qql_args.push(format!("QUERY '{}'", escape_qql_string(text)));
                }
            } else if let Some(sample) = qobj.get("sample") {
                if sample.is_object() {
                    qql_args.push("QUERY SAMPLE".to_string());
                }
            } else if qobj.contains_key("indices") {
                qql_args.push("QUERY '<sparse_query>' USING SPARSE".to_string());
            } else {
                qql_args.push("QUERY '<query>'".to_string());
            }
        }
    } else if let Some(v) = vector {
        if let Some(_arr) = v.as_array() {
            let vec_str = format_vector(v).unwrap_or_default();
            qql_args.push(format!("QUERY {}", vec_str));
        }
    } else {
        qql_args.push("QUERY '<query>'".to_string());
    }

    qql_args.push(format!("FROM {}", collection));

    // Limit
    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        qql_args.push(format!("LIMIT {}", limit));
    }

    // Offset
    if let Some(offset) = obj.get("offset").and_then(|v| v.as_i64()) {
        qql_args.push(format!("OFFSET {}", offset));
    }

    // Using
    if let Some(using) = obj.get("using").and_then(|v| v.as_str()) {
        qql_args.push(using_str(using));
    }

    // Filter
    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            qql_args.push(format!("WHERE {}", filter_str));
        }
    }

    // Score threshold
    if let Some(threshold) = obj.get("score_threshold").and_then(|v| v.as_f64()) {
        qql_args.push(format!("SCORE THRESHOLD {}", threshold));
    }

    // Group by
    if let Some(group_by) = obj.get("group_by").and_then(|v| v.as_str()) {
        qql_args.push(format!("GROUP BY '{}'", escape_qql_string(group_by)));
        if let Some(group_size) = obj.get("group_size").and_then(|v| v.as_i64()) {
            qql_args.push(format!("GROUP_SIZE {}", group_size));
        }
    }

    // Lookup from
    if let Some(lookup) = obj.get("lookup_from").and_then(|v| v.as_object()) {
        if let Some(s) = lookup_from_str(lookup) {
            qql_args.push(s);
        }
    }

    parts.push(qql_args.join(" "));
    Ok(parts)
}

fn convert_recommend(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid recommend JSON".to_string()),
    };

    let (positive, negative, strategy) = extract_recommend_params(obj);

    let positive_str = format_id_list(&positive);
    let mut with_parts = vec![format!("positive = ({})", positive_str)];
    if !negative.is_empty() {
        let negative_str = format_id_list(&negative);
        with_parts.push(format!("negative = ({})", negative_str));
    }

    let mut parts = vec![format!(
        "QUERY RECOMMEND WITH ({}) FROM {}",
        with_parts.join(", "),
        collection
    )];

    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        parts.push(format!("LIMIT {}", limit));
    }

    if let Some(s) = strategy {
        parts.push(format!("STRATEGY '{}'", escape_qql_string(&s)));
    }

    if let Some(using) = obj.get("using").and_then(|v| v.as_str()) {
        parts.push(using_str(using));
    }

    if let Some(lookup) = obj.get("lookup_from").and_then(|v| v.as_object()) {
        if let Some(s) = lookup_from_str(lookup) {
            parts.push(s);
        }
    }

    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            parts.push(format!("WHERE {}", filter_str));
        }
    }

    Ok(vec![parts.join(" ")])
}

fn extract_recommend_params(
    obj: &serde_json::Map<String, Value>,
) -> (Vec<Value>, Vec<Value>, Option<String>) {
    // Check top-level
    let mut positive = obj
        .get("positive")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut negative = obj
        .get("negative")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut strategy = obj
        .get("strategy")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Check nested query.recommend
    if let Some(query) = obj.get("query").and_then(|v| v.as_object()) {
        if let Some(recommend) = query.get("recommend").and_then(|v| v.as_object()) {
            if positive.is_empty() {
                positive = recommend
                    .get("positive")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
            }
            if negative.is_empty() {
                negative = recommend
                    .get("negative")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
            }
            if strategy.is_none() {
                strategy = recommend
                    .get("strategy")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
        }
    }

    (positive, negative, strategy)
}

fn convert_discover(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid discover JSON".to_string()),
    };

    let (target, ctx_pairs) = extract_discover_params(obj);

    let mut parts = Vec::new();
    if let Some(t) = target {
        parts.push(format!("QUERY DISCOVER TARGET {}", format_id(&t)));
        if !ctx_pairs.is_empty() {
            let pairs: Vec<String> = ctx_pairs
                .iter()
                .map(|(p, n)| format!("({}, {})", format_id(p), format_id(n)))
                .collect();
            parts.push(format!("CONTEXT PAIRS {}", pairs.join(", ")));
        }
    } else {
        parts.push("QUERY CONTEXT".to_string());
        if !ctx_pairs.is_empty() {
            let pairs: Vec<String> = ctx_pairs
                .iter()
                .map(|(p, n)| format!("({}, {})", format_id(p), format_id(n)))
                .collect();
            parts.push(format!("PAIRS {}", pairs.join(", ")));
        }
    }

    parts.push(format!("FROM {}", collection));

    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        parts.push(format!("LIMIT {}", limit));
    }

    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            parts.push(format!("WHERE {}", filter_str));
        }
    }

    Ok(vec![parts.join(" ")])
}

fn extract_discover_params(
    obj: &serde_json::Map<String, Value>,
) -> (Option<Value>, Vec<(Value, Value)>) {
    let target = obj.get("target").cloned();
    let ctx_pairs = extract_context_pairs(obj);

    // Check nested query.discover
    if let Some(query) = obj.get("query").and_then(|v| v.as_object()) {
        if let Some(discover) = query.get("discover").and_then(|v| v.as_object()) {
            let nested_target = discover.get("target").cloned();
            let nested_pairs = if let Some(ctx) = discover.get("context").and_then(|v| v.as_array())
            {
                ctx.iter()
                    .filter_map(|pair| {
                        pair.as_object().and_then(|p| {
                            let pos = p.get("positive")?.clone();
                            let neg = p.get("negative")?.clone();
                            Some((pos, neg))
                        })
                    })
                    .collect()
            } else {
                vec![]
            };
            if nested_target.is_some() || !nested_pairs.is_empty() {
                return (nested_target.or(target), nested_pairs);
            }
        }
        // Check query.context
        if let Some(ctx) = query.get("context").and_then(|v| v.as_array()) {
            let qpairs: Vec<(Value, Value)> = ctx
                .iter()
                .filter_map(|pair| {
                    pair.as_object().and_then(|p| {
                        let pos = p.get("positive")?.clone();
                        let neg = p.get("negative")?.clone();
                        Some((pos, neg))
                    })
                })
                .collect();
            if !qpairs.is_empty() {
                return (None, qpairs);
            }
        }
    }

    (target, ctx_pairs)
}

fn extract_context_pairs(obj: &serde_json::Map<String, Value>) -> Vec<(Value, Value)> {
    if let Some(ctx) = obj.get("context").and_then(|v| v.as_array()) {
        ctx.iter()
            .filter_map(|pair| {
                pair.as_object().and_then(|p| {
                    let pos = p.get("positive")?.clone();
                    let neg = p.get("negative")?.clone();
                    Some((pos, neg))
                })
            })
            .collect()
    } else {
        vec![]
    }
}

fn convert_scroll(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid scroll JSON".to_string()),
    };

    let mut parts = vec![format!("SCROLL FROM {}", collection)];

    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            parts.push(format!("WHERE {}", filter_str));
        }
    }

    if let Some(after) = obj.get("offset") {
        parts.push(format!("AFTER {}", format_id(after)));
    }

    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        parts.push(format!("LIMIT {}", limit));
    }

    Ok(vec![parts.join(" ")])
}

fn convert_get_points(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid get points JSON".to_string()),
    };
    let ids = match obj.get("ids").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Err("no ids in get points payload".to_string()),
    };

    let stmts: Vec<String> = ids
        .iter()
        .map(|id| format!("SELECT * FROM {} WHERE id = {}", collection, format_id(id)))
        .collect();

    Ok(stmts)
}

fn convert_delete_points(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid delete JSON".to_string()),
    };

    if let Some(_filter) = obj.get("filter") {
        return convert_delete_by_filter(input, collection);
    }

    let points = match obj.get("points").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return Err("no points in delete payload".to_string()),
    };

    let stmts: Vec<String> = points
        .iter()
        .map(|id| format!("DELETE FROM {} WHERE id = {}", collection, format_id(id)))
        .collect();
    Ok(stmts)
}

fn convert_delete_by_filter(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid delete filter JSON".to_string()),
    };

    let filter = match obj.get("filter") {
        Some(f) => f,
        None => return Err("no filter in delete by filter payload".to_string()),
    };

    let filter_str = convert_filter_to_qql(filter)?;
    Ok(vec![format!(
        "DELETE FROM {} WHERE {}",
        collection, filter_str
    )])
}

fn convert_set_payload(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid set payload JSON".to_string()),
    };

    let payload = match obj.get("payload").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return Err("no payload in set payload JSON".to_string()),
    };

    let payload_str = build_payload_dict(payload);

    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        return Ok(vec![format!(
            "UPDATE {} SET PAYLOAD = {} WHERE {}",
            collection, payload_str, filter_str
        )]);
    }

    if let Some(points) = obj.get("points").and_then(|v| v.as_array()) {
        let stmts: Vec<String> = points
            .iter()
            .map(|id| {
                format!(
                    "UPDATE {} SET PAYLOAD = {} WHERE id = {}",
                    collection,
                    payload_str,
                    format_id(id)
                )
            })
            .collect();
        return Ok(stmts);
    }

    Err("set payload requires points or filter".to_string())
}

fn convert_create_collection(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid create collection JSON".to_string()),
    };

    let vectors = obj.get("vectors").or_else(|| obj.get("vectors_config"));

    let mut stmt = format!("CREATE COLLECTION {}", collection);

    if let Some(Value::Object(map)) = vectors {
        let mut vec_defs = Vec::new();
        if map.contains_key("size") {
            vec_defs.push(build_vector_def("dense", map));
        } else {
            let mut names: Vec<&String> = map.keys().collect();
            names.sort();
            for name in names {
                if let Some(vec_obj) = map[name].as_object() {
                    vec_defs.push(build_vector_def(name, vec_obj));
                }
            }
        }
        if !vec_defs.is_empty() {
            stmt.push_str(" (\n    ");
            stmt.push_str(&vec_defs.join(",\n    "));
            stmt.push_str("\n)");
        }
    }

    Ok(vec![stmt])
}

fn build_vector_def(name: &str, v: &serde_json::Map<String, Value>) -> String {
    let size = v
        .get("size")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "?".to_string());
    let distance = v
        .get("distance")
        .map(|d| d.to_string())
        .unwrap_or_else(|| "Cosine".to_string());
    let mut def = format!("'{}' VECTOR({}, {})", name, size, distance);

    if let Some(mvc) = v.get("multivector_config").and_then(|v| v.as_object()) {
        if let Some(comp) = mvc.get("comparator") {
            def.push_str(&format!(" WITH MULTIVECTOR (comparator = '{}')", comp));
        }
    }

    if let Some(hnsw) = v.get("hnsw_config").and_then(|v| v.as_object()) {
        if let Some(m) = hnsw.get("m") {
            def.push_str(&format!(" WITH HNSW (m = {})", m));
        }
    }

    def
}

fn convert_create_index(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid create index JSON".to_string()),
    };

    let field = obj
        .get("field_name")
        .map(|v| format!("{}", v))
        .unwrap_or_else(|| "?".to_string());

    let schema = obj
        .get("field_schema")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Object(m) => m
                .get("type")
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "keyword".to_string()),
            _ => "keyword".to_string(),
        })
        .unwrap_or_else(|| "keyword".to_string());

    Ok(vec![format!(
        "CREATE INDEX ON COLLECTION {} FOR {} TYPE {}",
        collection, field, schema
    )])
}

fn convert_formula_query(input: &Value, collection: &str) -> Result<Vec<String>, String> {
    let obj = match input.as_object() {
        Some(o) => o,
        None => return Err("invalid formula query JSON".to_string()),
    };

    if let Some(searches) = obj.get("searches").and_then(|v| v.as_array()) {
        let mut all_stmts = Vec::new();
        for search in searches {
            let sub = convert_by_structure(search, collection)?;
            all_stmts.extend(sub);
        }
        return Ok(all_stmts);
    }

    let mut stmt_parts = Vec::new();
    stmts_with_prefetch(obj, collection, &mut stmt_parts, 0)?;

    // Base query part
    if let Some(query) = obj.get("query").and_then(|v| v.as_object()) {
        if let Some(text) = query.get("text").and_then(|v| v.as_str()) {
            stmt_parts.push(format!("QUERY '{}'", escape_qql_string(text)));
        } else if let Some(nearest) = query.get("nearest") {
            if let Some(arr) = nearest.as_array() {
                let vec_str = format_vector(&Value::Array(arr.clone())).unwrap_or_default();
                stmt_parts.push(format!("QUERY {}", vec_str));
            } else if let Some(doc_map) = nearest.as_object() {
                if let Some(text) = doc_map.get("text").and_then(|v| v.as_str()) {
                    stmt_parts.push(format!("QUERY '{}'", escape_qql_string(text)));
                } else {
                    stmt_parts.push("QUERY '<query>'".to_string());
                }
            } else {
                stmt_parts.push("QUERY '<query>'".to_string());
            }
        } else if query.contains_key("fusion") {
            // Fusion-only query
            let fusion = query
                .get("fusion")
                .and_then(|v| v.as_str())
                .unwrap_or("rrf");
            stmt_parts.push(format!("FUSION {}", fusion.to_uppercase()));
        }
    } else {
        stmt_parts.push("QUERY '<query>'".to_string());
    }

    stmt_parts.push(format!("FROM {}", collection));

    if let Some(limit) = obj.get("limit").and_then(|v| v.as_i64()) {
        stmt_parts.push(format!("LIMIT {}", limit));
    }

    if let Some(offset) = obj.get("offset").and_then(|v| v.as_i64()) {
        stmt_parts.push(format!("OFFSET {}", offset));
    }

    // Filter
    if let Some(filter) = obj.get("filter") {
        let filter_str = convert_filter_to_qql(filter)?;
        if !filter_str.is_empty() {
            stmt_parts.push(format!("WHERE {}", filter_str));
        }
    }

    // Using
    if let Some(using) = obj.get("using").and_then(|v| v.as_str()) {
        stmt_parts.push(using_str(using));
    }

    // Score Threshold
    if let Some(threshold) = obj.get("score_threshold").and_then(|v| v.as_f64()) {
        stmt_parts.push(format!("SCORE THRESHOLD {}", threshold));
    }

    Ok(vec![stmt_parts.join(" ")])
}

fn stmts_with_prefetch(
    obj: &serde_json::Map<String, Value>,
    _collection: &str,
    stmt_parts: &mut Vec<String>,
    depth: usize,
) -> Result<(), String> {
    if let Some(prefetch) = obj.get("prefetch") {
        let prefetches = match prefetch {
            Value::Array(arr) => arr.clone(),
            Value::Object(_) => vec![prefetch.clone()],
            _ => return Ok(()),
        };

        for (i, pf) in prefetches.iter().enumerate() {
            let cte_name = format!("_pf{}", depth * 100 + i);
            let pf_obj = match pf.as_object() {
                Some(o) => o,
                None => continue,
            };

            let mut pf_parts = Vec::new();
            stmts_with_prefetch(pf_obj, _collection, &mut pf_parts, depth + 1)?;

            if let Some(query) = pf_obj.get("query") {
                if let Some(qobj) = query.as_object() {
                    if let Some(text) = qobj.get("text").and_then(|v| v.as_str()) {
                        pf_parts.push(format!("QUERY '{}'", escape_qql_string(text)));
                    }
                } else if let Some(s) = query.as_str() {
                    pf_parts.push(format!("QUERY '{}'", escape_qql_string(s)));
                }
            } else if let Some(document) = pf_obj.get("document").and_then(|v| v.as_object()) {
                if let Some(text) = document.get("text").and_then(|v| v.as_str()) {
                    pf_parts.push(format!("QUERY '{}'", escape_qql_string(text)));
                }
            } else if let Some(vector) = pf_obj.get("vector").and_then(|v| v.as_array()) {
                if !vector.is_empty() {
                    let vs: Vec<String> = vector.iter().map(|v| v.to_string()).collect();
                    pf_parts.push(format!("QUERY [{}]", vs.join(", ")));
                }
            }

            if let Some(limit) = pf_obj.get("limit").and_then(|v| v.as_i64()) {
                pf_parts.push(format!("LIMIT {}", limit));
            }

            if let Some(filter) = pf_obj.get("filter") {
                let filter_str = convert_filter_to_qql(filter)?;
                if !filter_str.is_empty() {
                    pf_parts.push(format!("WHERE {}", filter_str));
                }
            }

            if let Some(threshold) = pf_obj.get("score_threshold").and_then(|v| v.as_f64()) {
                pf_parts.push(format!("SCORE THRESHOLD {}", threshold));
            }

            if let Some(using) = pf_obj.get("using").and_then(|v| v.as_str()) {
                pf_parts.push(using_str(using));
            }

            if pf_parts.is_empty() {
                pf_parts.push("QUERY ''".to_string());
            }

            let cte_str = pf_parts.join(" ");
            stmt_parts.push(format!("WITH {} AS ({})", cte_name, cte_str));
        }

        let refs: Vec<String> = (0..prefetches.len())
            .map(|i| format!("_pf{}", depth * 100 + i))
            .collect();
        stmt_parts.push(format!("PREFETCH ({})", refs.join(", ")));
    }

    Ok(())
}

// ── Structure detection helpers ────────────────────────────────

fn has_prefetch_and_query(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("query") && obj.get("query").and_then(|v| v.as_object()).is_some() {
        return Some(convert_formula_query(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_top_level_prefetch(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("prefetch") {
        return Some(convert_formula_query(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_batch_search(obj: &serde_json::Map<String, Value>) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("searches") {
        return Some(convert_formula_query(
            &Value::Object(obj.clone()),
            "unknown",
        ));
    }
    None
}

fn has_set_payload(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("payload") && (obj.contains_key("points") || obj.contains_key("filter")) {
        return Some(convert_set_payload(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_points_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if let Some(points) = obj.get("points").and_then(|v| v.as_array()) {
        if !points.is_empty() {
            if let Some(first) = points.first().and_then(|v| v.as_object()) {
                if first.contains_key("vector") || first.contains_key("payload") {
                    return Some(convert_upsert(&Value::Object(obj.clone()), collection));
                }
            }
            return Some(convert_delete_points(
                &Value::Object(obj.clone()),
                collection,
            ));
        }
    }
    None
}

fn has_vector_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("vector") {
        return Some(convert_search(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_positive_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("positive") {
        return Some(convert_recommend(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_target_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("target") {
        return Some(convert_discover(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_ids_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("ids") {
        return Some(convert_get_points(&Value::Object(obj.clone()), collection));
    }
    None
}

fn has_vectors_config(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("vectors") || obj.contains_key("vectors_config") {
        return Some(convert_create_collection(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_field_name(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("field_name") {
        return Some(convert_create_index(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}

fn has_filter_field(
    obj: &serde_json::Map<String, Value>,
    collection: &str,
) -> Option<Result<Vec<String>, String>> {
    if obj.contains_key("filter") {
        if obj.contains_key("limit") {
            return Some(convert_scroll(&Value::Object(obj.clone()), collection));
        }
        return Some(convert_delete_by_filter(
            &Value::Object(obj.clone()),
            collection,
        ));
    }
    None
}
