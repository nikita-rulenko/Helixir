//! Read-only installation readiness reports.

use serde::Serialize;

/// Status of one doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Required component is ready.
    Pass,
    /// Optional component is unavailable or degraded.
    Warn,
    /// Component was not selected by the install plan.
    Skipped,
    /// Required component is not ready.
    Fail,
}

/// One stable, machine-readable readiness check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    /// Stable check identifier.
    pub name: String,
    /// Result status.
    pub status: CheckStatus,
    /// Human-readable explanation.
    pub detail: String,
    /// Whether failure blocks installer success.
    pub required: bool,
}

/// Complete doctor result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    /// All checks in deterministic order.
    pub checks: Vec<DoctorCheck>,
    /// True only when no required check failed.
    pub ready: bool,
}

/// Inputs gathered by platform detectors; this struct has no side effects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoctorInputs {
    /// Whether the installed binary/assets are present.
    pub binaries: Option<bool>,
    /// Whether central config exists and has protected permissions.
    pub config: Option<bool>,
    /// Whether backend health/schema/persistence passed.
    pub backend: Option<bool>,
    /// Whether selected local LLM is ready.
    pub llm: Option<bool>,
    /// Whether selected embedding model is ready.
    pub embeddings: Option<bool>,
    /// Whether the required Nomic embedding model is ready through Ollama.
    pub nomic: Option<bool>,
    /// Whether Nomic is the selected path or was activated as recovery.
    pub nomic_required: bool,
    /// Whether the required NLI judge is ready.
    pub nli: Option<bool>,
    /// Whether MCP initialize/list-tools smoke passed.
    pub mcp: Option<bool>,
    /// Whether selected clients see `helixir-local`.
    pub clients: Option<bool>,
}

impl DoctorReport {
    /// Build a report from read-only detector inputs.
    #[must_use]
    pub fn from_inputs(inputs: &DoctorInputs) -> Self {
        let mut checks = Vec::new();
        push_bool(
            &mut checks,
            "binaries",
            inputs.binaries,
            true,
            "installed runtime assets",
        );
        push_bool(
            &mut checks,
            "config",
            inputs.config,
            true,
            "central config and permissions",
        );
        push_bool(
            &mut checks,
            "backend",
            inputs.backend,
            true,
            "backend health/schema/persistence",
        );
        push_bool(
            &mut checks,
            "llm",
            inputs.llm,
            true,
            "selected LLM readiness",
        );
        push_bool(
            &mut checks,
            "embeddings",
            inputs.embeddings,
            true,
            "embedding endpoint readiness",
        );
        push_bool(
            &mut checks,
            "nomic",
            inputs.nomic,
            inputs.nomic_required,
            "nomic-embed-text availability",
        );
        push_bool(
            &mut checks,
            "nli",
            inputs.nli,
            true,
            "required local NLI judge",
        );
        push_bool(
            &mut checks,
            "mcp",
            inputs.mcp,
            true,
            "MCP initialize/list-tools smoke",
        );
        push_bool(
            &mut checks,
            "clients",
            inputs.clients,
            true,
            "selected MCP client registrations",
        );
        let ready = checks
            .iter()
            .all(|check| !check.required || check.status != CheckStatus::Fail);
        Self { checks, ready }
    }

    /// Serialize the stable JSON report.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

fn push_bool(
    checks: &mut Vec<DoctorCheck>,
    name: &str,
    value: Option<bool>,
    required: bool,
    detail: &str,
) {
    let (status, suffix) = match value {
        Some(true) => (CheckStatus::Pass, "ready"),
        Some(false) => (CheckStatus::Fail, "not ready"),
        None if required => (CheckStatus::Fail, "not checked"),
        None => (CheckStatus::Skipped, "not selected"),
    };
    checks.push(DoctorCheck {
        name: name.to_string(),
        status,
        detail: format!("{detail}: {suffix}"),
        required,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_report_is_ready_and_json_stable() {
        let inputs = DoctorInputs {
            binaries: Some(true),
            config: Some(true),
            backend: Some(true),
            llm: Some(true),
            embeddings: Some(true),
            nomic: Some(true),
            nomic_required: true,
            nli: Some(true),
            mcp: Some(true),
            clients: Some(true),
        };
        let report = DoctorReport::from_inputs(&inputs);
        assert!(report.ready);
        assert!(report.to_json().unwrap().contains("\"ready\": true"));
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == CheckStatus::Pass)
        );
    }

    #[test]
    fn required_failure_and_missing_nli_block_readiness() {
        let report = DoctorReport::from_inputs(&DoctorInputs {
            binaries: Some(true),
            config: Some(true),
            backend: Some(false),
            llm: Some(true),
            embeddings: Some(true),
            nomic: Some(true),
            nomic_required: true,
            nli: None,
            mcp: Some(true),
            clients: Some(true),
        });
        assert!(!report.ready);
        assert!(
            report
                .checks
                .iter()
                .any(|check| { check.name == "backend" && check.status == CheckStatus::Fail })
        );
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "nli" && check.status == CheckStatus::Fail)
        );
    }

    #[test]
    fn healthy_remote_embeddings_do_not_require_preinstalled_nomic() {
        let report = DoctorReport::from_inputs(&DoctorInputs {
            binaries: Some(true),
            config: Some(true),
            backend: Some(true),
            llm: Some(true),
            embeddings: Some(true),
            nomic: None,
            nomic_required: false,
            nli: Some(true),
            mcp: Some(true),
            clients: Some(true),
        });
        assert!(report.ready);
        assert!(report.checks.iter().any(|check| {
            check.name == "nomic" && check.status == CheckStatus::Skipped && !check.required
        }));
    }
}
