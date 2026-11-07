//! Prompts and generic GitHub (Worker D): prompts-chat, github-direct.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

pub mod github_direct;
pub mod prompts_chat;

pub use github_direct::{GithubDirect, RepoTarget};
pub use prompts_chat::PromptsChat;

pub fn providers() -> Vec<Arc<dyn MarketplaceProvider>> {
    vec![Arc::new(PromptsChat::new()), Arc::new(GithubDirect::new())]
}
