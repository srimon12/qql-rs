use napi_derive::napi;
use qql_core::lexer::Lexer;
use qql_core::parser::Parser;

#[napi]
pub fn parse(input: String) -> napi::Result<String> {
    match Parser::parse(&input) {
        Ok(stmt) => Ok(format!("{:#?}", stmt)),
        Err(e) => Err(napi::Error::from_reason(e.to_string())),
    }
}

#[napi]
pub fn tokenize(input: String) -> napi::Result<String> {
    let lexer = Lexer::new(&input);
    let mut tokens = Vec::new();
    for token_result in lexer {
        let token = token_result.map_err(|e| napi::Error::from_reason(e.to_string()))?;
        tokens.push(serde_json::json!({
            "kind": token.kind.as_str(),
            "text": token.text,
            "pos": token.pos,
        }));
    }
    serde_json::to_string(&tokens)
        .map_err(|e| napi::Error::from_reason(format!("failed to serialize tokens: {}", e)))
}
