use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("unknown resource kind `{0}`")]
    UnknownKind(String),
    #[error("invalid resource id `{0}`: expected `kind:provider:key[#variant]`")]
    InvalidResourceId(String),
    #[error("invalid provider id `{0}`: use lowercase ASCII letters, digits, `-`, `_` or `.`")]
    InvalidProviderId(String),
    #[error("unsafe relative path `{path}`: {reason}")]
    UnsafeRelativePath { path: String, reason: &'static str },
}
