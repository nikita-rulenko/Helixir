//! Model-runtime adapters used by the onboarding planner.
//!
//! The adapter emits argv vectors rather than shell snippets.  This keeps the
//! interactive CLI and a future native UI on the same safe command contract.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// A command that can be displayed, tested, and executed without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Executable resolved from PATH or an absolute path.
    pub program: String,
    /// Arguments passed verbatim to the executable.
    pub args: Vec<String>,
}

impl CommandSpec {
    /// Construct a command specification.
    #[must_use]
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

/// Ollama command adapter.
pub struct OllamaAdapter;

impl OllamaAdapter {
    /// Probe command used to detect an installed Ollama binary.
    #[must_use]
    pub fn version() -> CommandSpec {
        CommandSpec::new("ollama", ["--version"])
    }

    /// List locally available models.
    #[must_use]
    pub fn list() -> CommandSpec {
        CommandSpec::new("ollama", ["list"])
    }

    /// Start the local API service.
    #[must_use]
    pub fn serve() -> CommandSpec {
        CommandSpec::new("ollama", ["serve"])
    }

    /// Pull one model by its exact user-selected name.
    #[must_use]
    pub fn pull(model: &str) -> CommandSpec {
        CommandSpec::new("ollama", ["pull", model])
    }

    /// Parse the tabular `ollama list` output without trusting model metadata.
    #[must_use]
    pub fn parse_models(output: &str) -> BTreeSet<String> {
        output
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// NLI download locations are kept in one value so the executor can report a
/// precise rollback target and a UI can show the planned files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NliInstallTarget {
    /// Directory containing model.onnx, tokenizer.json and config.json.
    pub directory: PathBuf,
    /// Immutable HuggingFace revision used for downloads.
    pub revision: String,
}

impl NliInstallTarget {
    /// Construct a target for the default model directory.
    #[must_use]
    pub fn default_target(directory: PathBuf, revision: impl Into<String>) -> Self {
        Self {
            directory,
            revision: revision.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_commands_are_shell_free_and_deterministic() {
        assert_eq!(
            OllamaAdapter::version(),
            CommandSpec::new("ollama", ["--version"])
        );
        assert_eq!(
            OllamaAdapter::pull("nomic-embed-text:latest"),
            CommandSpec::new("ollama", ["pull", "nomic-embed-text:latest"])
        );
        assert!(!OllamaAdapter::pull("x; rm -rf /").args.is_empty());
    }

    #[test]
    fn model_parser_skips_header_and_empty_rows() {
        let models = OllamaAdapter::parse_models(
            "NAME ID SIZE MODIFIED\nllama3.2:3b abc 2GB now\n\n n\tdef 1GB now\n",
        );
        assert!(models.contains("llama3.2:3b"));
        assert!(models.contains("n"));
        assert_eq!(models.len(), 2);
    }
}
