use crate::ddl::{lower_alter_collection, lower_create_collection, lower_create_index};
use crate::mutation::{
    lower_delete_request, lower_scroll_request, lower_update_payload_request,
    lower_update_vector_request, lower_upsert_request,
};
use crate::query::lower_query_request;
use crate::types::*;
use qql_core::ast::{QueryCollection, QueryExpr, Stmt};

pub enum RequestBody {
    Query(Box<QueryRequest>),
    Points(PointsRequest),
    Scroll(ScrollRequest),
    Upsert(UpsertRequest),
    Delete(DeleteRequest),
    UpdateVector(UpdateVectorRequest),
    UpdatePayload(UpdatePayloadRequest),
    CreateCollection(CreateCollectionRequest),
    CreateIndex(CreateIndexRequest),
}

impl RequestBody {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            RequestBody::Query(r) => serde_json::to_value(r).unwrap(),
            RequestBody::Points(r) => serde_json::to_value(r).unwrap(),
            RequestBody::Scroll(r) => serde_json::to_value(r).unwrap(),
            RequestBody::Upsert(r) => serde_json::to_value(r).unwrap(),
            RequestBody::Delete(r) => serde_json::to_value(r).unwrap(),
            RequestBody::UpdateVector(r) => serde_json::to_value(r).unwrap(),
            RequestBody::UpdatePayload(r) => serde_json::to_value(r).unwrap(),
            RequestBody::CreateCollection(r) => serde_json::to_value(r).unwrap(),
            RequestBody::CreateIndex(r) => serde_json::to_value(r).unwrap(),
        }
    }
}

pub struct Route {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Option<RequestBody>,
}

impl Route {
    pub fn body_json(&self) -> Option<serde_json::Value> {
        self.body.as_ref().map(|b| b.to_json())
    }
}

pub fn route(statement: &Stmt) -> Route {
    match statement {
        Stmt::Query(query) => {
            let collection = match &query.collection {
                QueryCollection::Explicit(name) => name.clone(),
                QueryCollection::Inherited => String::new(),
            };

            if matches!(query.expression, QueryExpr::Points { .. }) {
                let ids = match &query.expression {
                    QueryExpr::Points { ids } => ids,
                    _ => unreachable!(),
                };
                let id_list: Vec<_> = ids
                    .iter()
                    .map(|id| match id {
                        qql_core::ast::PointId::Number(n) => {
                            serde_json::Value::Number((*n).into())
                        }
                        qql_core::ast::PointId::String(s) => {
                            serde_json::Value::String(s.clone())
                        }
                    })
                    .collect();
                let with_payload = query.output.payload.as_ref().map(|p| match p {
                    qql_core::ast::PayloadSelector::All => PayloadSelectorReq::All(true),
                    qql_core::ast::PayloadSelector::None => PayloadSelectorReq::All(false),
                    qql_core::ast::PayloadSelector::Include(fields) => {
                        PayloadSelectorReq::Include {
                            include: fields.clone(),
                        }
                    }
                    qql_core::ast::PayloadSelector::Exclude(fields) => {
                        PayloadSelectorReq::Exclude {
                            exclude: fields.clone(),
                        }
                    }
                });
                let with_vector = query.output.vectors.as_ref().map(|v| match v {
                    qql_core::ast::VectorSelector::All => VectorSelectorReq::All(true),
                    qql_core::ast::VectorSelector::None => VectorSelectorReq::All(false),
                    qql_core::ast::VectorSelector::Names(names) => {
                        VectorSelectorReq::Names(names.clone())
                    }
                });

                Route {
                    method: Method::Post,
                    path: format!("/collections/{}/points", collection),
                    query: Vec::new(),
                    body: Some(RequestBody::Points(PointsRequest {
                        ids: id_list,
                        with_payload,
                        with_vector,
                    })),
                }
            } else {
                Route {
                    method: Method::Post,
                    path: format!("/collections/{}/points/query", collection),
                    query: Vec::new(),
                    body: Some(RequestBody::Query(Box::new(lower_query_request(query)))),
                }
            }
        }
        Stmt::Scroll(scroll) => Route {
            method: Method::Post,
            path: format!("/collections/{}/points/scroll", scroll.collection),
            query: Vec::new(),
            body: Some(RequestBody::Scroll(lower_scroll_request(
                scroll.limit,
                scroll.filter.as_deref(),
                scroll.after.as_ref(),
            ))),
        },
        Stmt::Upsert(upsert) => {
            let mut query = Vec::new();
            if upsert.embedding.is_some() {
                query.push(("wait".into(), "true".into()));
            }
            Route {
                method: Method::Put,
                path: format!("/collections/{}/points", upsert.collection),
                query,
                body: Some(RequestBody::Upsert(lower_upsert_request(upsert))),
            }
        }
        Stmt::Delete(delete) => Route {
            method: Method::Post,
            path: format!("/collections/{}/points/delete", delete.collection),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::Delete(lower_delete_request(delete))),
        },
        Stmt::UpdateVector(update) => Route {
            method: Method::Put,
            path: format!("/collections/{}/points/vectors", update.collection),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::UpdateVector(lower_update_vector_request(
                update,
            ))),
        },
        Stmt::UpdatePayload(update) => Route {
            method: Method::Post,
            path: format!("/collections/{}/points/payload", update.collection),
            query: vec![("wait".into(), "true".into())],
            body: Some(RequestBody::UpdatePayload(lower_update_payload_request(
                update,
            ))),
        },
        Stmt::CreateCollection(create) => Route {
            method: Method::Put,
            path: format!("/collections/{}", create.collection),
            query: Vec::new(),
            body: Some(RequestBody::CreateCollection(lower_create_collection(
                create,
            ))),
        },
        Stmt::AlterCollection(alter) => Route {
            method: Method::Patch,
            path: format!("/collections/{}", alter.collection),
            query: Vec::new(),
            body: Some(RequestBody::CreateCollection(lower_alter_collection(alter))),
        },
        Stmt::DropCollection(drop) => Route {
            method: Method::Delete,
            path: format!("/collections/{}", drop.collection),
            query: Vec::new(),
            body: None,
        },
        Stmt::CreateIndex(index) => Route {
            method: Method::Put,
            path: format!("/collections/{}/index", index.collection),
            query: Vec::new(),
            body: Some(RequestBody::CreateIndex(lower_create_index(index))),
        },
        Stmt::ShowCollections => Route {
            method: Method::Get,
            path: "/collections".into(),
            query: Vec::new(),
            body: None,
        },
        Stmt::ShowCollection(collection) => Route {
            method: Method::Get,
            path: format!("/collections/{}", collection),
            query: Vec::new(),
            body: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qql_core::parser::Parser;

    #[test]
    fn query_routes_correctly() {
        let s = Parser::parse("QUERY 'hello' FROM docs LIMIT 10;").unwrap();
        let r = route(&s);
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points/query");
        assert!(r.body.is_some());
    }

    #[test]
    fn points_lookup() {
        let s = Parser::parse("QUERY POINTS (42) FROM docs WITH PAYLOAD true;").unwrap();
        let r = route(&s);
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.path, "/collections/docs/points");
    }

    #[test]
    fn upsert_with_embedding_waits() {
        let s =
            Parser::parse("UPSERT INTO docs VALUES {id: 1, text: 'x'} USING DENSE MODEL 'm';")
                .unwrap();
        let r = route(&s);
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
    }

    #[test]
    fn delete_has_wait() {
        let s = Parser::parse("DELETE FROM docs WHERE id = 1;").unwrap();
        let r = route(&s);
        assert!(r.query.iter().any(|(k, v)| k == "wait" && v == "true"));
    }

    #[test]
    fn show_collections_no_body() {
        let s = Parser::parse("SHOW COLLECTIONS;").unwrap();
        let r = route(&s);
        assert_eq!(r.method, Method::Get);
        assert!(r.body.is_none());
    }

    #[test]
    fn all_endpoint_methods() {
        let cases = [
            ("QUERY 'x' FROM docs;", Method::Post, "/collections/docs/points/query"),
            ("SCROLL FROM docs LIMIT 10;", Method::Post, "/collections/docs/points/scroll"),
            ("UPSERT INTO docs VALUES {id: 1, title: 'x'};", Method::Put, "/collections/docs/points"),
            ("DELETE FROM docs WHERE id = 1;", Method::Post, "/collections/docs/points/delete"),
            ("UPDATE docs SET VECTOR = [0.1] WHERE id = 'x';", Method::Put, "/collections/docs/points/vectors"),
            ("UPDATE docs SET PAYLOAD = {x: 1} WHERE id = 1;", Method::Post, "/collections/docs/points/payload"),
            ("CREATE COLLECTION docs (d VECTOR(4, DOT));", Method::Put, "/collections/docs"),
            ("ALTER COLLECTION docs WITH HNSW (m = 16);", Method::Patch, "/collections/docs"),
            ("DROP COLLECTION docs;", Method::Delete, "/collections/docs"),
            ("CREATE INDEX ON COLLECTION docs FOR title TYPE text;", Method::Put, "/collections/docs/index"),
            ("SHOW COLLECTIONS;", Method::Get, "/collections"),
            ("SHOW COLLECTION docs;", Method::Get, "/collections/docs"),
        ];
        for (source, method, path) in cases {
            let s = Parser::parse(source).unwrap();
            let r = route(&s);
            assert_eq!(r.method, method, "method mismatch for: {}", source);
            assert_eq!(r.path, path, "path mismatch for: {}", source);
        }
    }
}
