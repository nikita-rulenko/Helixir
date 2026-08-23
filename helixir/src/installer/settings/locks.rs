//! Environment-owned settings that the control plane must not overwrite.

use super::SettingsPatch;

pub(super) fn locked_fields() -> Vec<String> {
    [
        ("HELIXIR_MODE", "mode"),
        ("HELIX_LLM_PROVIDER", "reasoning_provider"),
        ("HELIX_LLM_MODEL", "reasoning_model"),
        ("HELIX_LLM_API_KEY", "reasoning_api_key"),
        ("HELIX_LLM_BASE_URL", "reasoning_base_url"),
        ("HELIX_EMBEDDING_PROVIDER", "embedding_provider"),
        ("HELIX_EMBEDDING_MODEL", "embedding_model"),
        ("HELIX_EMBEDDING_URL", "embedding_url"),
        ("HELIX_EMBEDDING_API_KEY", "embedding_api_key"),
        ("HELIXIR_GATEWAY_PUBLIC_URL", "gateway_public_url"),
    ]
    .into_iter()
    .filter(|(variable, _)| std::env::var_os(variable).is_some())
    .map(|(_, field)| field.to_string())
    .collect()
}

pub(super) fn reject_locked_fields(patch: &SettingsPatch) -> anyhow::Result<()> {
    let locked = locked_fields();
    let requested = [
        ("mode", patch.mode.is_some()),
        ("reasoning_provider", patch.reasoning_provider.is_some()),
        ("reasoning_model", patch.reasoning_model.is_some()),
        ("reasoning_api_key", patch.reasoning_api_key.is_some()),
        ("reasoning_base_url", patch.reasoning_base_url.is_some()),
        ("embedding_provider", patch.embedding_provider.is_some()),
        ("embedding_model", patch.embedding_model.is_some()),
        ("embedding_url", patch.embedding_url.is_some()),
        ("embedding_api_key", patch.embedding_api_key.is_some()),
        ("gateway_public_url", patch.gateway_public_url.is_some()),
    ];
    if let Some((field, _)) = requested
        .into_iter()
        .find(|(field, present)| *present && locked.iter().any(|locked| locked == field))
    {
        anyhow::bail!("{field} is controlled by the host environment");
    }
    Ok(())
}
