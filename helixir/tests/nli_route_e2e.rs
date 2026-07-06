//! #96 Lever 2 guard: the NLI edge router builds a typed SUPPORTS edge with
//! NO working LLM at all — the amplifier thesis in one test. A weak (here:
//! dead) model still gets a reasoning graph, because the router types the
//! entailment pair locally before the LLM would ever be consulted.
//!
//! Skips (with a message) when the NLI model is not installed — release
//! artifacts are lean by design.
//!
//! ```text
//! HELIX_E2E=1 HELIXIR_RETRIEVAL_PROFILE=algo_opt HELIX_LLM_API_KEY=dead-key-on-purpose \
//!   cargo test -p helixir --test nli_route_e2e -- --ignored --nocapture
//! ```

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use helixir::core::HelixirClient;
use helixir::llm::extractor::ExtractedMemory;

fn atom(text: &str) -> ExtractedMemory {
    ExtractedMemory {
        text: text.to_string(),
        memory_type: "fact".to_string(),
        certainty: 90,
        importance: 60,
        entities: vec![],
        context: None,
    }
}

#[tokio::test]
#[ignore = "needs HELIX_E2E=1 + live HelixDB + embeddings + the NLI model (no LLM needed)"]
async fn nli_routes_a_typed_edge_with_a_dead_llm() {
    assert_eq!(std::env::var("HELIX_E2E").unwrap_or_default(), "1");

    #[cfg(not(feature = "nli"))]
    {
        println!("SKIP: built without the nli feature");
        return;
    }
    #[cfg(feature = "nli")]
    {
        let model_dir = helixir::llm::nli::NliJudge::default_dir();
        if !model_dir.join("model.onnx").exists() {
            println!("SKIP: NLI model not installed at {}", model_dir.display());
            return;
        }

        let client = HelixirClient::from_env().expect("client");
        client.initialize().await.expect("init");

        let run = format!(
            "{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let user = format!("nliroute_{}", &run[..10]);

        // A same-subject NEGATION pair: negations barely move embeddings, so
        // the pair lands in the 0.70–0.98 gray zone (below the dedup gate,
        // above the recall floor) and reaches Phase D — where the NLI judge
        // types it CONTRADICTS locally. The LLM key is dead on purpose: with
        // no working model, only the router can create this edge.
        let a = format!(
            "NLITEST{run}: the nightly export job always retries failed uploads three times."
        );
        let b = format!(
            "NLITEST{run}: uploads that fail during the nightly export are never retried — \
             the job drops them immediately and moves on."
        );

        let r1 = client
            .add_prepared(vec![atom(&a)], &user, None, Some("e2e-nli"))
            .await
            .expect("first write");
        assert_eq!(r1.memories_added, 1);
        tokio::time::sleep(Duration::from_secs(2)).await; // snapshot lag

        let r2 = client
            .add_prepared(vec![atom(&b)], &user, None, Some("e2e-nli"))
            .await
            .expect("second write");
        assert_eq!(
            r2.memories_added, 1,
            "the restatement must ADD (not dedup) for the edge to matter: {r2:?}"
        );
        assert!(
            r2.relations_created >= 1,
            "NLI must route at least one typed edge with a dead LLM (relations_created={}) — \
             the amplifier contract: a graph even for a model that can't reason",
            r2.relations_created
        );

        println!(
            "==== nli_route_e2e ====\n{} relation(s) created with a dead LLM key",
            r2.relations_created
        );
    }
}
