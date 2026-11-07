//! Skill catalogues (Worker A): lobehub, skillssh, skillsmp,
//! voltagent-skills, officialskills.

use std::sync::Arc;

use perkolator_marketplace::MarketplaceProvider;

mod common;
mod curated;
pub mod lobehub;
pub mod officialskills;
pub mod skillsmp;
pub mod skillssh;
pub mod voltagent;

pub use lobehub::LobeHub;
pub use officialskills::OfficialSkills;
pub use skillsmp::SkillsMp;
pub use skillssh::SkillsSh;
pub use voltagent::VoltAgentSkills;

pub fn providers() -> Vec<Arc<dyn MarketplaceProvider>> {
    vec![
        Arc::new(LobeHub::new()),
        Arc::new(SkillsSh::new()),
        Arc::new(SkillsMp::new()),
        Arc::new(VoltAgentSkills::new()),
        Arc::new(OfficialSkills::new()),
    ]
}
