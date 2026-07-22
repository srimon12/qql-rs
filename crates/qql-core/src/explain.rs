use crate::ast::{CollectionMode, QueryCollection, QueryExpr, QueryInput, Stmt};
use crate::error::QqlError;
use crate::parser::Parser;
use alloc::format;
use alloc::string::String;

pub fn explain(source: &str) -> Result<String, QqlError> {
    let statement = Parser::parse(source)?;
    Ok(explain_node(&statement))
}

pub fn explain_node(statement: &Stmt) -> String {
    let mut output = String::new();
    match statement {
        Stmt::Query(query) => {
            output.push_str("Statement: QUERY\n");
            output.push_str(&format!("Intent: {}\n", query_intent(&query.expression)));
            match &query.collection {
                QueryCollection::Explicit(collection) => {
                    output.push_str(&format!("Collection: {}\n", collection));
                }
                QueryCollection::Inherited => output.push_str("Collection: inherited\n"),
            }
            if !query.ctes.is_empty() {
                output.push_str(&format!("CTEs: {}\n", query.ctes.len()));
            }
            if query.filter.is_some() {
                output.push_str("Filter: present\n");
            }
            if let Some(limit) = query.page.limit {
                output.push_str(&format!("Limit: {}\n", limit));
            }
        }
        Stmt::Scroll(statement) => output.push_str(&format!(
            "Statement: SCROLL\nCollection: {}\nLimit: {}\n",
            statement.collection, statement.limit
        )),
        Stmt::Upsert(statement) => output.push_str(&format!(
            "Statement: UPSERT\nCollection: {}\nPoints: {}\n",
            statement.collection,
            statement.points.len()
        )),
        Stmt::CreateCollection(statement) => {
            let mode = match statement.mode {
                CollectionMode::Dense { .. } => "dense",
                CollectionMode::Hybrid { .. } => "hybrid",
                CollectionMode::Rerank => "rerank-oriented",
            };
            output.push_str(&format!(
                "Statement: CREATE COLLECTION\nCollection: {}\nDeclared mode: {}\n",
                statement.collection, mode
            ));
        }
        Stmt::CreateIndex(statement) => output.push_str(&format!(
            "Statement: CREATE INDEX\nCollection: {}\nField: {}\n",
            statement.collection, statement.field
        )),
        Stmt::DropIndex(statement) => output.push_str(&format!(
            "Statement: DROP INDEX\nCollection: {}\nField: {}\n",
            statement.collection, statement.field
        )),
        Stmt::Count(statement) => {
            output.push_str("Statement: COUNT\n");
            output.push_str(&format!("Collection: {}\n", statement.collection));
            if statement.filter.is_some() {
                output.push_str("Filter: present\n");
            }
        }
        Stmt::AlterCollection(statement) => output.push_str(&format!(
            "Statement: ALTER COLLECTION\nCollection: {}\n",
            statement.collection
        )),
        Stmt::DropCollection(statement) => output.push_str(&format!(
            "Statement: DROP COLLECTION\nCollection: {}\n",
            statement.collection
        )),
        Stmt::ShowCollections => output.push_str("Statement: SHOW COLLECTIONS\n"),
        Stmt::ShowCollection(collection) => {
            output.push_str(&format!(
                "Statement: SHOW COLLECTION\nCollection: {}\n",
                collection
            ));
        }
        Stmt::Delete(statement) => output.push_str(&format!(
            "Statement: DELETE\nCollection: {}\nSelector: typed point selector\n",
            statement.collection
        )),
        Stmt::ClearPayload(statement) => output.push_str(&format!(
            "Statement: CLEAR PAYLOAD\nCollection: {}\nSelector: typed point selector\n",
            statement.collection
        )),
        Stmt::DeleteVector(statement) => output.push_str(&format!(
            "Statement: DELETE VECTOR\nCollection: {}\nVectors: {:?}\nSelector: typed point selector\n",
            statement.collection, statement.vector_names
        )),
        Stmt::UpdateVector(statement) => output.push_str(&format!(
            "Statement: UPDATE VECTOR\nCollection: {}\n",
            statement.collection
        )),
        Stmt::UpdatePayload(statement) => output.push_str(&format!(
            "Statement: UPDATE PAYLOAD\nCollection: {}\n",
            statement.collection
        )),
    }
    output
}

fn query_intent(expression: &QueryExpr) -> &'static str {
    match expression {
        QueryExpr::Points { .. } => "retrieve points by ID",
        QueryExpr::Nearest { mmr: Some(_), .. } => "maximal marginal relevance (MMR) search",
        QueryExpr::Nearest { input, .. } => match input {
            QueryInput::Text { .. } => "nearest neighbors from text",
            QueryInput::Vector(_) => "nearest neighbors from a vector",
            QueryInput::Point(_) => "nearest neighbors from a point",
        },
        QueryExpr::Recommend { .. } => "recommend from positive and negative examples",
        QueryExpr::Context { .. } => "context search",
        QueryExpr::Discover { .. } => "discovery search",
        QueryExpr::OrderBy { .. } => "payload order query",
        QueryExpr::SampleRandom => "random sample",
        QueryExpr::Fusion { .. } => "fuse prefetched result sets",
        QueryExpr::Formula { .. } => "formula-based scoring",
        QueryExpr::RelevanceFeedback { .. } => "relevance feedback",
        QueryExpr::Hybrid { .. } => "hybrid shorthand",
        QueryExpr::Rerank { .. } => "explicit prefetched rerank",
    }
}
