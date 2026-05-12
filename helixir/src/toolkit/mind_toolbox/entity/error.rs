//! Errors raised by the entity subsystem.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum EntityError {
    #[error("Entity not found: {0}")]
    NotFound(String),
    #[error("Entity already exists: {0}")]
    AlreadyExists(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Invalid entity: {0}")]
    Invalid(String),
    #[error("Validation error: {0}")]
    Validation(String),
}
