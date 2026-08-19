//! Client for the narrow native host supervisor.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, ensure};

#[derive(Clone)]
pub(super) struct SupervisorClient {
    base_url: Arc<str>,
    token: Arc<str>,
    client: reqwest::Client,
}

impl SupervisorClient {
    pub(super) fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(base_url) = std::env::var("HELIXIR_SUPERVISOR_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };
        let token_path = std::env::var_os("HELIXIR_SUPERVISOR_TOKEN_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/secrets/helixir-supervisor-token"));
        let token = std::fs::read_to_string(&token_path)
            .with_context(|| format!("read supervisor token {}", token_path.display()))?;
        let token = token.trim().to_string();
        ensure!(token.len() >= 32, "supervisor token file is invalid");
        let base_url = base_url.trim_end_matches('/').to_string();
        reqwest::Url::parse(&base_url).context("parse HELIXIR_SUPERVISOR_URL")?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("build supervisor HTTP client")?;
        Ok(Some(Self {
            base_url: Arc::from(base_url),
            token: Arc::from(token),
            client,
        }))
    }

    pub(super) async fn discovery(&self) -> anyhow::Result<crate::installer::SystemState> {
        self.get("/v1/discovery").await
    }

    pub(super) async fn health(&self) -> anyhow::Result<crate::agents::hygieia::HealthSnapshot> {
        self.get("/v1/health").await
    }

    pub(super) async fn plan(
        &self,
        options: &crate::installer::InstallOptions,
    ) -> anyhow::Result<crate::installer::InstallPlan> {
        let response = self
            .client
            .post(format!("{}/v1/install/plan", self.base_url))
            .bearer_auth(self.token.as_ref())
            .json(options)
            .send()
            .await
            .context("call host supervisor plan")?;
        decode(response).await
    }

    pub(super) async fn apply(
        &self,
        options: &crate::installer::InstallOptions,
    ) -> anyhow::Result<crate::installer::InstallReport> {
        self.post("/v1/install/apply", options).await
    }

    pub(super) async fn start_install(
        &self,
        options: &crate::installer::InstallOptions,
    ) -> anyhow::Result<crate::installer::operations::OperationSnapshot> {
        self.post("/v1/install/operations", options).await
    }

    pub(super) async fn resume_install(
        &self,
        operation_id: &str,
        options: &crate::installer::InstallOptions,
    ) -> anyhow::Result<crate::installer::operations::OperationSnapshot> {
        self.post(
            &format!("/v1/install/operations/{operation_id}/resume"),
            options,
        )
        .await
    }

    pub(super) async fn install_status(
        &self,
        operation_id: &str,
    ) -> anyhow::Result<crate::installer::operations::OperationSnapshot> {
        self.get(&format!("/v1/install/operations/{operation_id}"))
            .await
    }

    pub(super) async fn install_events(
        &self,
        operation_id: &str,
        after: u64,
    ) -> anyhow::Result<crate::installer::operations::OperationEventBatch> {
        self.get(&format!(
            "/v1/install/operations/{operation_id}/events?after={after}"
        ))
        .await
    }

    pub(super) async fn verify(&self) -> anyhow::Result<serde_json::Value> {
        self.post("/v1/install/verify", &serde_json::json!({}))
            .await
    }

    pub(super) async fn operation(
        &self,
        request: &crate::installer::supervisor::HostOperation,
    ) -> anyhow::Result<crate::installer::supervisor::HostOperationResult> {
        self.post("/v1/operations/run", request).await
    }

    pub(super) async fn settings(
        &self,
    ) -> anyhow::Result<crate::installer::settings::SettingsSnapshot> {
        self.get("/v1/settings").await
    }

    pub(super) async fn apply_settings(
        &self,
        patch: &crate::installer::settings::SettingsPatch,
    ) -> anyhow::Result<crate::installer::supervisor_admin::SettingsMutationReceipt> {
        self.post("/v1/settings", patch).await
    }

    pub(super) async fn backups(
        &self,
    ) -> anyhow::Result<crate::installer::backups::BackupInventory> {
        self.get("/v1/backups").await
    }

    pub(super) async fn create_backup(
        &self,
    ) -> anyhow::Result<crate::installer::backups::BackupReceipt> {
        self.post("/v1/backups/create", &serde_json::json!({}))
            .await
    }

    pub(super) async fn verify_backup(
        &self,
        request: &crate::installer::supervisor_admin::BackupIdRequest,
    ) -> anyhow::Result<crate::installer::backups::BackupReceipt> {
        self.post("/v1/backups/verify", request).await
    }

    pub(super) async fn restore_backup(
        &self,
        request: &crate::installer::backups::RestoreRequest,
    ) -> anyhow::Result<crate::installer::backups::BackupReceipt> {
        self.post("/v1/backups/restore", request).await
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let response = self
            .client
            .get(format!("{}{}", self.base_url, path))
            .bearer_auth(self.token.as_ref())
            .send()
            .await
            .context("call host supervisor")?;
        decode(response).await
    }

    async fn post<T: serde::de::DeserializeOwned, B: serde::Serialize + Sync>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        let response = self
            .client
            .post(format!("{}{}", self.base_url, path))
            .bearer_auth(self.token.as_ref())
            .json(body)
            .send()
            .await
            .context("call host supervisor")?;
        decode(response).await
    }
}

async fn decode<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> anyhow::Result<T> {
    let status = response.status();
    if status.is_success() {
        return response.json().await.context("decode supervisor response");
    }
    let problem = response
        .json::<crate::installer::supervisor::SupervisorProblem>()
        .await
        .ok();
    anyhow::bail!(
        "host supervisor returned {status}: {}",
        problem
            .map(|value| value.message)
            .unwrap_or_else(|| "request failed".to_string())
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_secret_path_is_not_a_host_home_mount() {
        let path = std::path::Path::new("/run/secrets/helixir-supervisor-token");
        assert!(path.is_absolute());
        assert!(!path.starts_with("/home"));
    }
}
