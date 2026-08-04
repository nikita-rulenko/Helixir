use super::{HelixirConfig, MemoryMode};

#[test]
fn test_from_env_reads_llm_base_url() {
    // #15: temp_env scopes + serializes env mutation — no unsafe, no races.
    temp_env::with_var("HELIX_LLM_BASE_URL", Some("http://localhost:11434"), || {
        let config = HelixirConfig::from_env();
        assert_eq!(
            config.llm_base_url.as_deref(),
            Some("http://localhost:11434")
        );
    });
}

#[test]
fn test_default_has_no_base_url() {
    let config = HelixirConfig::default();
    assert!(config.llm_base_url.is_none());
}

#[test]
fn test_fallback_chain_env_parses_and_empty_clears() {
    temp_env::with_var(
        "HELIX_LLM_FALLBACK_CHAIN",
        Some(" deepseek , ollama "),
        || {
            let config = HelixirConfig::from_env();
            assert_eq!(config.llm_fallback_chain, vec!["deepseek", "ollama"]);
        },
    );
    temp_env::with_var("HELIX_LLM_FALLBACK_CHAIN", Some(""), || {
        let config = HelixirConfig::from_env();
        assert!(
            config.llm_fallback_chain.is_empty(),
            "explicit empty value must clear the chain"
        );
    });
}

#[test]
fn test_from_env_reads_embedding_url() {
    // Set a recognizable URL different from the ollama default so the
    // assertion catches a regression where embedding_url is shadowed.
    temp_env::with_var(
        "HELIX_EMBEDDING_URL",
        Some("https://openrouter.ai/api/v1"),
        || {
            let config = HelixirConfig::from_env();
            assert_eq!(config.embedding_url, "https://openrouter.ai/api/v1");
        },
    );
}

#[test]
fn memory_mode_defaults_to_solo_and_never_silently_escalates() {
    assert_eq!(HelixirConfig::default().mode, MemoryMode::Solo);
    assert_eq!(MemoryMode::parse(""), MemoryMode::Solo);
    assert_eq!(MemoryMode::parse("nonsense"), MemoryMode::Solo);
    assert_eq!(MemoryMode::parse("personal"), MemoryMode::Solo);
    assert_eq!(MemoryMode::parse("Collective"), MemoryMode::Collective);
    assert_eq!(MemoryMode::parse("hive"), MemoryMode::Collective);
    assert_eq!(MemoryMode::parse("insights"), MemoryMode::Insights);
    assert_eq!(MemoryMode::parse(" FULL "), MemoryMode::Insights);
}

#[test]
fn partial_toml_overrides_only_named_fields() {
    // A partial file mentions one nested knob; everything else stays default.
    let toml = r#"
            [moira.clotho]
            dominance_margin = 0.99

            [retrieval.ppr]
            alpha = 0.4
        "#;
    let cfg: HelixirConfig = toml::from_str(toml).expect("partial toml parses");
    assert_eq!(cfg.moira.clotho.dominance_margin, 0.99); // overridden
    assert_eq!(cfg.retrieval.ppr.alpha, 0.4); // overridden
    // Untouched fields keep their defaults at every level:
    assert_eq!(cfg.moira.atropos.min_hops, 2);
    assert_eq!(cfg.moira.clotho.grow_threshold, 0.62);
    assert_eq!(cfg.retrieval.ppr.max_iterations, 20);
    assert_eq!(cfg.retrieval.graph.edge_weights.because, 1.0);
    assert_eq!(cfg.host, "localhost");
    assert_eq!(cfg.swarm.active_window_secs, 90);
    assert_eq!(cfg.mode, MemoryMode::Solo);
}

#[test]
fn config_defaults_match_audited_hardcode() {
    let c = HelixirConfig::default();
    assert_eq!(c.retrieval.ppr.alpha, 0.6);
    assert_eq!(c.retrieval.rank_decay, 0.92);
    assert_eq!(c.moira.lachesis.subset_pmi_bar, 0.5);
    assert_eq!(c.moira.atropos.quality_pmi_bar, 1.0);
    assert_eq!(c.write.cross_user_link_certainty, 80);
    assert_eq!(c.ingest.max_retries, 5);
    assert_eq!(c.retry.max, 3);
    assert_eq!(c.gateway.default_bind, "0.0.0.0:8765");
    assert!(c.gateway.auth_token.is_none());
}

#[test]
fn gateway_auth_can_be_enabled_in_partial_config() {
    let cfg: HelixirConfig = toml::from_str(
        r#"
                [gateway]
                default_bind = "127.0.0.1:9876"
                auth_token = "test-token"
            "#,
    )
    .expect("gateway config parses");
    assert_eq!(cfg.gateway.default_bind, "127.0.0.1:9876");
    assert_eq!(cfg.gateway.auth_token.as_deref(), Some("test-token"));
}

#[test]
fn gateway_token_env_enables_auth_and_empty_value_disables_it() {
    temp_env::with_var("HELIXIR_GATEWAY_TOKEN", Some("env-token"), || {
        let cfg = HelixirConfig::from_env();
        assert_eq!(cfg.gateway.auth_token.as_deref(), Some("env-token"));
    });
    temp_env::with_var("HELIXIR_GATEWAY_TOKEN", Some(""), || {
        let cfg = HelixirConfig::from_env();
        assert!(cfg.gateway.auth_token.is_none());
    });
}

#[test]
fn memory_mode_capabilities_are_tiered() {
    assert!(!MemoryMode::Solo.collective_enabled());
    assert!(!MemoryMode::Solo.insights_enabled());
    assert!(MemoryMode::Collective.collective_enabled());
    assert!(!MemoryMode::Collective.insights_enabled());
    assert!(MemoryMode::Insights.collective_enabled());
    assert!(MemoryMode::Insights.insights_enabled());
}
