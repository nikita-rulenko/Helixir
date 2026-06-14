//! The Moirai — Helixir's background agents (the daemon's inner life, #42).
//!
//! Helixir is no longer only an MCP server; it is an agent whose MCP surface is
//! one part. The Moirai are its inner processes:
//!
//! - **Clotho** — the Spinner: tags memories from a controlled vocabulary so
//!   shared tags weave distant memories into subsets.
//! - **Lachesis** — the Measurer: routes chains within subsets and gates them
//!   against apophenia (coherence gate — first increment landed).
//! - **Atropos** — the Cutter: curates surviving chains into an insight journal.
//!   *(not built yet)*
//!
//! Each agent is a library that **composes toolkit primitives** into behavior.
//! Dependencies flow `agents → toolkit`, never the reverse — the toolkit knows
//! nothing about agents.

pub mod clotho;
pub mod lachesis;
