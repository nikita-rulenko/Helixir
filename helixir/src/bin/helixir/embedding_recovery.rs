use super::*;

pub(crate) fn configured_embedding_choice(
    config: &helixir::core::config::HelixirConfig,
) -> Result<helixir::installer::EmbeddingChoice> {
    if config.embedding_provider.eq_ignore_ascii_case("ollama") {
        return Ok(helixir::installer::EmbeddingChoice::LocalOllamaNomic);
    }
    let api_key = config
        .embedding_api_key
        .clone()
        .filter(|key| !key.trim().is_empty())
        .context("remote embedding API key is missing")?;
    anyhow::ensure!(
        !config.embedding_provider.trim().is_empty(),
        "remote embedding provider is missing"
    );
    anyhow::ensure!(
        !config.embedding_model.trim().is_empty(),
        "remote embedding model is missing"
    );
    let parsed =
        reqwest::Url::parse(&config.embedding_url).context("remote embedding URL is invalid")?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "http" | "https"),
        "remote embedding URL must use http or https"
    );
    Ok(helixir::installer::EmbeddingChoice::Remote(
        helixir::installer::RemoteEmbeddingConfig {
            provider: config.embedding_provider.to_lowercase(),
            model: config.embedding_model.clone(),
            url: config.embedding_url.trim_end_matches('/').to_string(),
            api_key,
        },
    ))
}

pub(crate) async fn probe_embedding_choice(
    choice: &helixir::installer::EmbeddingChoice,
) -> Result<()> {
    let (provider, model, base_url, api_key) = match choice {
        helixir::installer::EmbeddingChoice::LocalOllamaNomic => (
            "ollama".to_string(),
            helixir::DEFAULT_EMBEDDING_MODEL.to_string(),
            helixir::DEFAULT_OLLAMA_URL.to_string(),
            None,
        ),
        helixir::installer::EmbeddingChoice::Remote(remote) => (
            remote.provider.clone(),
            remote.model.clone(),
            remote.url.clone(),
            Some(remote.api_key.clone()),
        ),
    };
    let generator = helixir::EmbeddingGenerator::new(helixir::llm::EmbeddingConfig {
        provider,
        base_url,
        model,
        api_key,
        timeout_secs: 15,
        cache_size: 8,
        cache_ttl: 60,
        fallback_enabled: false,
        fallback_url: helixir::DEFAULT_OLLAMA_URL.to_string(),
        fallback_model: helixir::DEFAULT_EMBEDDING_MODEL.to_string(),
    });
    let vector = generator
        .generate("helixir doctor embedding readiness probe", false)
        .await
        .context("embedding provider probe failed")?;
    anyhow::ensure!(
        !vector.is_empty(),
        "embedding provider returned an empty vector"
    );
    anyhow::ensure!(
        vector.iter().all(|value| value.is_finite()),
        "embedding provider returned a non-finite vector"
    );
    Ok(())
}

pub(crate) async fn repair_embeddings_with_local_fallback(
    executor: &OnboardExecutor,
    reason: &str,
) -> Result<()> {
    eprintln!("doctor: selected embeddings are not ready: {reason}");
    eprintln!("doctor: activating recovery with Ollama + nomic-embed-text");

    let (installed, _, _) = detect_ollama().await;
    if !installed {
        eprintln!("doctor: installing Ollama");
        executor.install_ollama().await?;
    }
    executor.start_ollama().await?;

    let models =
        helixir::installer::models::OllamaAdapter::list_api(helixir::DEFAULT_OLLAMA_URL).await?;
    if !helixir::installer::models::OllamaAdapter::has_model(
        &models,
        helixir::DEFAULT_EMBEDDING_MODEL,
    ) {
        eprintln!("doctor: downloading nomic-embed-text");
        helixir::installer::models::OllamaAdapter::pull_and_verify(
            helixir::DEFAULT_OLLAMA_URL,
            helixir::DEFAULT_EMBEDDING_MODEL,
            3,
        )
        .await?;
    }
    write_local_embedding_config()?;
    probe_embedding_choice(&helixir::installer::EmbeddingChoice::LocalOllamaNomic).await?;
    executor.mark_embedding_repaired();
    eprintln!("doctor: embedding recovery completed; central config now uses Ollama/Nomic");
    Ok(())
}

fn write_local_embedding_config() -> Result<()> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    let path = home.join(".helixir/helixir.toml");
    let patch = helixir::installer::config::ConfigPatch::default()
        .set("embedding_provider", "ollama")
        .set("embedding_model", helixir::DEFAULT_EMBEDDING_MODEL)
        .set("embedding_url", helixir::DEFAULT_OLLAMA_URL);
    helixir::installer::config::write_patch(&path, &patch)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn fake_embedding_server(status: &str, body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let body = body.to_string();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        format!("http://{address}/v1")
    }

    fn remote_choice(url: String) -> helixir::installer::EmbeddingChoice {
        helixir::installer::EmbeddingChoice::Remote(helixir::installer::RemoteEmbeddingConfig {
            provider: "openai".to_string(),
            model: "test-embedding".to_string(),
            url,
            api_key: "secret".to_string(),
        })
    }

    #[tokio::test]
    async fn remote_probe_requires_a_real_embedding_response() {
        let ready = remote_choice(fake_embedding_server(
            "200 OK",
            r#"{"data":[{"embedding":[0.1,0.2]}]}"#,
        ));
        probe_embedding_choice(&ready).await.unwrap();

        let broken = remote_choice(fake_embedding_server(
            "401 Unauthorized",
            r#"{"error":"bad key"}"#,
        ));
        assert!(probe_embedding_choice(&broken).await.is_err());
    }

    #[test]
    fn configured_remote_requires_a_key_and_local_maps_to_nomic() {
        let local = helixir::core::config::HelixirConfig::default();
        assert!(matches!(
            configured_embedding_choice(&local).unwrap(),
            helixir::installer::EmbeddingChoice::LocalOllamaNomic
        ));

        let mut remote = local;
        remote.embedding_provider = "openai".to_string();
        remote.embedding_model = "text-embedding-3-small".to_string();
        remote.embedding_url = "https://example.invalid/v1".to_string();
        remote.embedding_api_key = None;
        assert!(configured_embedding_choice(&remote).is_err());
    }
}
