//! Clotho's starter dictionary — the controlled vocabulary the Spinner tags
//! against. English-canonical (other languages attach via `ALIAS_OF` later).
//! Deliberately small and broad; the daemon grows it via charter-gated
//! escalation.
//!
//! `(name, kind, description, parent)`. Names are lowercase so they equal the
//! normalized form `ensure_category` stores. The `parent` chain is the
//! in-memory hierarchy auto-tagging propagates over (a memory tagged
//! `agriculture` also gets `raw material`). Note: the DB `SUBCATEGORY_OF` edges
//! are not yet read at query time — routing rides direct tags — so this table
//! is today the single source of hierarchy truth.

pub(super) const CATEGORY_SEEDS: &[(&str, &str, &str, Option<&str>)] = &[
    (
        "raw material",
        "domain",
        "primary commodities and feedstocks — grown, mined or extracted before downstream processing",
        None,
    ),
    (
        "agriculture",
        "domain",
        "farming, crops, grain, harvests, soil, irrigation and monsoon-dependent yields",
        Some("raw material"),
    ),
    (
        "petrochemicals",
        "domain",
        "oil, natural gas, fracking, refining and hydrocarbon-derived feedstocks and additives",
        Some("raw material"),
    ),
    (
        "minerals and metals",
        "domain",
        "ores, mining, metals and mineral commodities",
        Some("raw material"),
    ),
    (
        "food industry",
        "domain",
        "food production, processing, ingredients and additives such as thickeners and stabilisers",
        None,
    ),
    (
        "energy",
        "domain",
        "power generation, fuels, electricity and energy markets",
        None,
    ),
    (
        "finance",
        "domain",
        "markets, equities, share prices, trading, costs and commodity pricing",
        None,
    ),
    (
        "technology",
        "domain",
        "software, hardware, computing, algorithms and engineering",
        None,
    ),
    (
        "weather and climate",
        "domain",
        "weather, monsoon, rainfall, seasons and climate patterns",
        None,
    ),
    (
        "health and medicine",
        "domain",
        "health, disease, medicine, biology and clinical care",
        None,
    ),
    (
        "logistics and supply chain",
        "domain",
        "transport, shipping, warehousing and supply-and-demand flows",
        None,
    ),
    (
        "geography and regions",
        "domain",
        "places, regions, countries and geographic context",
        None,
    ),
];

/// Ancestors of `name` in the seed hierarchy, nearest-first. Empty for a root or
/// an unknown name. Bounded against accidental cycles in the table.
pub(super) fn ancestors(name: &str) -> Vec<&'static str> {
    let mut chain = Vec::new();
    let mut current = name.to_string();
    for _ in 0..CATEGORY_SEEDS.len() {
        let parent = CATEGORY_SEEDS
            .iter()
            .find(|(n, _, _, _)| *n == current)
            .and_then(|(_, _, _, p)| *p);
        match parent {
            Some(p) => {
                chain.push(p);
                current = p.to_string();
            }
            None => break,
        }
    }
    chain
}
