//! Deterministic synthetic documents for tests and benchmarks only.

use perkolator_core::{ProviderId, ResourceId, ResourceKind};

use crate::SearchDoc;

const WORDS: &[&str] = &[
    "git",
    "github",
    "review",
    "frontend",
    "design",
    "postgres",
    "docker",
    "rust",
    "python",
    "deploy",
    "lint",
    "format",
    "test",
    "browser",
    "search",
    "memory",
    "notion",
    "slack",
    "linear",
    "figma",
    "kubernetes",
    "terraform",
    "azure",
    "cloudflare",
    "vercel",
    "markdown",
    "pdf",
    "excel",
    "security",
    "audit",
    "refactor",
    "debug",
    "planner",
    "writer",
    "translate",
    "api",
    "schema",
    "graphql",
    "react",
    "vue",
    "swift",
    "kotlin",
    "shell",
    "release",
    "changelog",
];

const PUBLISHERS: &[&str] = &["anthropic", "vercel", "community", "acme", "local"];

/// Small deterministic xorshift generator so runs are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

pub fn docs(count: usize) -> Vec<SearchDoc> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let provider = ProviderId::new("synthetic").expect("valid provider id");
    (0..count)
        .map(|n| {
            let kind = ResourceKind::ALL[n % ResourceKind::ALL.len()];
            let name = format!("{}-{}-{n}", rng.pick(WORDS), rng.pick(WORDS));
            let summary = format!(
                "{} {} helper for {} and {} workflows",
                rng.pick(WORDS),
                rng.pick(WORDS),
                rng.pick(WORDS),
                rng.pick(WORDS)
            );
            SearchDoc {
                id: ResourceId::new(kind, provider.clone(), format!("/synthetic/{n}")),
                kind,
                name,
                publisher: Some(rng.pick(PUBLISHERS).to_owned()),
                tags: vec![rng.pick(WORDS).to_owned(), rng.pick(WORDS).to_owned()],
                summary,
                extra: vec![
                    format!("references/{}.md", rng.pick(WORDS)),
                    format!("scripts/{}.sh", rng.pick(WORDS)),
                ],
                installed: rng.next().is_multiple_of(4),
                remote: rng.next().is_multiple_of(3),
                source_label: "synthetic".to_owned(),
            }
        })
        .collect()
}

/// Representative launcher queries: exact words, prefixes, subsequences,
/// multi-word, kind prefixes and misses.
pub const QUERIES: &[&str] = &[
    "git",
    "gh",
    "review",
    "front",
    "fe des",
    "postgres",
    "dkr",
    "rust lint",
    "mcp: git",
    "skill: design",
    "@installed test",
    "brwsr",
    "memory",
    "kube",
    "tf",
    "cloud flare",
    "pdf",
    "sec audit",
    "zzzzqx",
    "release changelog",
];
