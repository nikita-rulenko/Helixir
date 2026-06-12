# Diagram sources

These are the [Mermaid](https://mermaid.js.org/) sources for the diagrams rendered in the
project `README.md`. GitHub renders Mermaid natively, so the README has no binary image
assets — edit the ```` ```mermaid ```` block directly in `README.md`, or edit the matching
file here and copy it back.

Render locally to validate before pushing:

```bash
npx @mermaid-js/mermaid-cli -i docs/diagrams/architecture.mmd -o /tmp/check.svg
```

| File | README section |
|:-----|:---------------|
| `how-it-works.mmd` | How It Works |
| `architecture.mmd` | Architecture |
| `read-path.mmd` | Read path (zero LLM calls) |
| `ontology.mmd` | Ontology hierarchy |

Theme: Helixir "elixir" palette — gold (`#b8860b`) on ink, plum (`#2b2440`) for the
graph+vector store.
