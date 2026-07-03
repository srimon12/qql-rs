use napi_derive::napi;
use qql_core::parser::Parser;

#[napi]
pub fn parse(input: String) -> napi::Result<String> {
    match Parser::parse(&input) {
        Ok(stmt) => Ok(format!("{:#?}", stmt)),
        Err(e) => Err(napi::Error::from_reason(e.to_string())),
    }
}
