#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    Str(&'a str),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    Dict(alloc::vec::Vec<(&'a str, Value<'a>)>),
    List(alloc::vec::Vec<Value<'a>>),
}
