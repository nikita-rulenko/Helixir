# v0.9.1 — The Honest Arsenal

> _Supersedes `v0.9.0`. Every declared edge type now either works, or is gone._

A patch with a law inside. The edge-type census found 37 declared arrow
types and only 22 alive — the rest were duplicate twins and abandoned
ambitions, three of them documented as active. This release makes the
arsenal honest and teaches everyone (models AND agents) how to use it.

## The law of the graph

An edge type earns existence only if: (1) a read-path algorithm walks it
to answer its own question class, (2) something reliably produces it, and
(3) without it the reader would need an LLM call. This is the amplifier
mechanism in one sentence: **the smart writer pays, the dumb reader walks
precomputed arrows — and looks smart.**

## What changed

- **12 dead edge types cut** from the schema (dupes and the never-built
  doc-pedagogy subsystem), with the live bench swapped in place — all
  memories intact. The cleanup exposed two latent bugs: the operator
  cascade-drop referenced a dead twin (fixed here) and the chunk-embedding
  path is broken on both ends (#86, design decision pending).
- **ALIAS_OF lives**: Clotho converges synonym categories at mint time and
  a capped, idempotent alias pass stitches existing near-duplicates —
  fragmented vocabularies blind Lachesis, and weak models fragment
  vocabularies. e2e-proven convergent.
- **Structural guarantees**: explicit "is part of" / "is a kind of"
  (EN + RU) now deterministically produce PART_OF / IS_A edges — same
  backstop mechanism that made BECAUSE reliable. 3/3 e2e.
- **Example-leak firewall (#79)**: an atom resembling a prompt's worked
  example AND ungrounded in the user's message is dropped — fabricated
  memories die at the gate; real users keep everything. Extraction now
  keeps the input's language (Russian in → Russian atoms).
- **Prompts teach writing for the graph**: explicit connectives are now a
  documented agent technique, not a lucky accident.
- New, calmer project mark.

## Upgrade

Drop-in for the binary. The schema swap ships in `schema/` — existing
installs: `helix check` + rebuild + container swap per the runbook (or
`install.sh` fresh). No data migration; removed types had zero instances.
