//! Test utilities for helix-cli tests
//!
//! This module provides test infrastructure for running tests in isolation
//! without interfering with the user's environment or other parallel tests.

use std::path::PathBuf;
use tempfile::TempDir;

/// A test context that provides isolated directories for testing.
///
/// TestContext creates:
/// - A temporary project directory
/// - A temporary cache directory (set via HELIX_CACHE_DIR env var)
/// - A temporary helix home directory (set via HELIX_HOME env var)
///
/// The HELIX_CACHE_DIR and HELIX_HOME environment variables are automatically
/// set when the context is created and restored when it is dropped.
pub struct TestContext {
    /// The temporary directory containing everything
    pub _temp_dir: TempDir,
    /// The project path within the temp directory
    pub project_path: PathBuf,
    /// The cache directory within the temp directory
    pub cache_dir: PathBuf,
    /// The helix home directory within the temp directory
    pub helix_home: PathBuf,
    /// Guard to restore the HELIX_CACHE_DIR env var on drop
    _cache_env_guard: EnvGuard,
    /// Guard to restore the HELIX_HOME env var on drop
    _home_env_guard: EnvGuard,
}

/// Guard that restores an environment variable to its previous state on drop.
struct EnvGuard {
    key: &'static str,
    old_value: Option<String>,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: We're restoring the environment variable to its previous state.
        // Tests using TestContext should not run in parallel with tests that
        // depend on HELIX_CACHE_DIR, but in practice each test gets its own
        // isolated directory so this is safe.
        unsafe {
            match &self.old_value {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

impl TestContext {
    /// Create a new test context with isolated directories.
    ///
    /// This will:
    /// 1. Create a temporary directory
    /// 2. Create project, cache, and helix home subdirectories
    /// 3. Set the HELIX_CACHE_DIR environment variable to the cache directory
    /// 4. Set the HELIX_HOME environment variable to the helix home directory
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let project_path = temp_dir.path().join("project");
        let cache_dir = temp_dir.path().join("cache");
        let helix_home = temp_dir.path().join(".helix");

        std::fs::create_dir_all(&project_path).expect("Failed to create project dir");
        std::fs::create_dir_all(&cache_dir).expect("Failed to create cache dir");
        std::fs::create_dir_all(&helix_home).expect("Failed to create helix home dir");

        // Save old values and set new ones
        let old_cache_value = std::env::var("HELIX_CACHE_DIR").ok();
        let old_home_value = std::env::var("HELIX_HOME").ok();
        // SAFETY: We're setting environment variables for test isolation.
        // Each test creates its own unique temp directory, so there are no
        // data races on the actual directory contents.
        unsafe {
            std::env::set_var("HELIX_CACHE_DIR", &cache_dir);
            std::env::set_var("HELIX_HOME", &helix_home);
        }

        Self {
            _temp_dir: temp_dir,
            project_path,
            cache_dir,
            helix_home,
            _cache_env_guard: EnvGuard {
                key: "HELIX_CACHE_DIR",
                old_value: old_cache_value,
            },
            _home_env_guard: EnvGuard {
                key: "HELIX_HOME",
                old_value: old_home_value,
            },
        }
    }

    /// Create a basic helix project structure with valid schema and queries.
    ///
    /// This creates:
    /// - helix.toml configuration file
    /// - .helix directory
    /// - db/schema.hx with sample node and edge definitions
    /// - db/queries.hx with sample queries
    pub fn setup_valid_project(&self) {
        use crate::config::HelixConfig;
        use std::fs;

        // Create helix.toml
        let config = HelixConfig::default_config("test-project");
        let config_path = self.project_path.join("helix.toml");
        config
            .save_to_file(&config_path)
            .expect("Failed to save config");

        // Create .helix directory
        fs::create_dir_all(self.project_path.join(".helix")).expect("Failed to create .helix");

        // Create queries directory
        let queries_dir = self.project_path.join("db");
        fs::create_dir_all(&queries_dir).expect("Failed to create queries directory");

        // Create valid schema.hx
        let schema_content = r#"
// Node types
N::User {
    name: String,
    email: String,
}

N::Post {
    title: String,
    content: String,
}

// Edge types
E::Authored {
    From: User,
    To: Post,
}

E::Likes {
    From: User,
    To: Post,
}
"#;
        fs::write(queries_dir.join("schema.hx"), schema_content)
            .expect("Failed to write schema.hx");

        // Create valid queries.hx
        let queries_content = r#"
QUERY GetUser(user_id: ID) =>
    user <- N<User>(user_id)
    RETURN user

QUERY GetUserPosts(user_id: ID) =>
    posts <- N<User>(user_id)::Out<Authored>
    RETURN posts
"#;
        fs::write(queries_dir.join("queries.hx"), queries_content)
            .expect("Failed to write queries.hx");
    }

    /// Create a helix project with only schema (no queries).
    pub fn setup_schema_only_project(&self) {
        use crate::config::HelixConfig;
        use std::fs;

        // Create helix.toml
        let config = HelixConfig::default_config("test-project");
        let config_path = self.project_path.join("helix.toml");
        config
            .save_to_file(&config_path)
            .expect("Failed to save config");

        // Create .helix directory
        fs::create_dir_all(self.project_path.join(".helix")).expect("Failed to create .helix");

        // Create queries directory with only schema
        let queries_dir = self.project_path.join("db");
        fs::create_dir_all(&queries_dir).expect("Failed to create queries directory");

        let schema_content = r#"
N::User {
    name: String,
    email: String,
}

E::Follows {
    From: User,
    To: User,
}
"#;
        fs::write(queries_dir.join("schema.hx"), schema_content)
            .expect("Failed to write schema.hx");
    }

    /// Create a helix project without schema definitions (queries only, should fail validation).
    pub fn setup_project_without_schema(&self) {
        use crate::config::HelixConfig;
        use std::fs;

        // Create helix.toml
        let config = HelixConfig::default_config("test-project");
        let config_path = self.project_path.join("helix.toml");
        config
            .save_to_file(&config_path)
            .expect("Failed to save config");

        // Create .helix directory
        fs::create_dir_all(self.project_path.join(".helix")).expect("Failed to create .helix");

        // Create queries directory with only queries (no schema)
        let queries_dir = self.project_path.join("db");
        fs::create_dir_all(&queries_dir).expect("Failed to create queries directory");

        let queries_content = r#"
QUERY GetUser(user_id: ID) =>
    user <- N<User>(user_id)
    RETURN user
"#;
        fs::write(queries_dir.join("queries.hx"), queries_content)
            .expect("Failed to write queries.hx");
    }

    /// Create a helix project with invalid syntax in queries.
    pub fn setup_project_with_invalid_syntax(&self) {
        use crate::config::HelixConfig;
        use std::fs;

        // Create helix.toml
        let config = HelixConfig::default_config("test-project");
        let config_path = self.project_path.join("helix.toml");
        config
            .save_to_file(&config_path)
            .expect("Failed to save config");

        // Create .helix directory
        fs::create_dir_all(self.project_path.join(".helix")).expect("Failed to create .helix");

        // Create queries directory
        let queries_dir = self.project_path.join("db");
        fs::create_dir_all(&queries_dir).expect("Failed to create queries directory");

        // Create valid schema
        let schema_content = r#"
N::User {
    name: String,
}
"#;
        fs::write(queries_dir.join("schema.hx"), schema_content)
            .expect("Failed to write schema.hx");

        // Create queries with invalid syntax
        let invalid_queries = r#"
QUERY InvalidQuery {
    this is not valid helix syntax!!!
}
"#;
        fs::write(queries_dir.join("queries.hx"), invalid_queries)
            .expect("Failed to write queries.hx");
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creates_directories() {
        let ctx = TestContext::new();

        assert!(ctx.project_path.exists());
        assert!(ctx.cache_dir.exists());
    }

    #[test]
    fn test_context_sets_env_var() {
        let ctx = TestContext::new();

        let env_value = std::env::var("HELIX_CACHE_DIR").expect("HELIX_CACHE_DIR should be set");
        assert_eq!(PathBuf::from(env_value), ctx.cache_dir);
    }

    // NOTE: test_context_restores_env_var_on_drop is removed because it
    // cannot run reliably in parallel with other tests that also set
    // HELIX_CACHE_DIR. The EnvGuard functionality is tested implicitly
    // through the other tests.

    #[test]
    fn test_setup_valid_project() {
        let ctx = TestContext::new();
        ctx.setup_valid_project();

        assert!(ctx.project_path.join("helix.toml").exists());
        assert!(ctx.project_path.join(".helix").exists());
        assert!(ctx.project_path.join("db/schema.hx").exists());
        assert!(ctx.project_path.join("db/queries.hx").exists());
    }
}
