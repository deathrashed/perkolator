//! One parser per resource kind. Each reads its native format and produces
//! normalised records through the shared [`crate::scan::Scanner`].

pub(crate) mod agents;
pub(crate) mod mcp;
pub(crate) mod plugins;
pub(crate) mod prompts;
pub(crate) mod rules;
pub(crate) mod skills;
