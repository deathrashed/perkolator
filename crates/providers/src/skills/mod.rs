//! Skill catalogues (Worker A): lobehub, skillssh, skillsmp,
//! voltagent-skills, officialskills.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

pub fn providers() -> Vec<Arc<dyn MarketplaceProvider>> {
    Vec::new()
}
