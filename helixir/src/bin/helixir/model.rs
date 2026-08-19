use super::*;

pub(crate) async fn model_cmd(sub: &ModelCmd) -> Result<()> {
    use helixir::llm::nli;
    match sub {
        ModelCmd::Which => {
            println!("host:                  {}", nli::host_label());
            println!("variant for this host: {}", nli::pick_onnx_variant());
            Ok(())
        }
        ModelCmd::Status => {
            let s = nli::status();
            println!("NLI model — host {}", s.host);
            println!("  dir:              {}", s.dir.display());
            println!("  installed:        {}", s.installed);
            if s.installed {
                println!("  model.onnx:       {:.1} MB", s.onnx_bytes as f64 / 1e6);
            }
            println!("  variant for host: {}", s.variant_for_host);
            if !s.installed {
                println!("\nRun `helixir model download` to fetch it (~90 MB).");
            }
            Ok(())
        }
        ModelCmd::Download { force } => {
            println!(
                "Downloading NLI model for {} (variant: {}) …",
                nli::host_label(),
                nli::pick_onnx_variant()
            );
            let bytes = nli::download(*force).await?;
            println!(
                "Fetched {:.1} MB into {}.\n",
                bytes as f64 / 1e6,
                nli::NliJudge::default_dir().display()
            );
            // Readiness immediately after install (agreed flow).
            nli_check()
        }
        ModelCmd::Check => nli_check(),
    }
}

pub(crate) fn nli_check() -> Result<()> {
    use helixir::llm::nli::{NliJudge, NliLabel};

    let dir = NliJudge::default_dir();
    println!("Local NLI judge — liveness + readiness check");
    println!("Loading from {} …\n", dir.display());
    let mut judge = NliJudge::load(&dir).context(
        "load NLI model (onboarding downloads the required judge to ~/.helixir/models/nli)",
    )?;
    // Introspected, not assumed — this is what bit us before.
    println!("  model inputs : {:?}", judge.input_names());
    println!("  model outputs: {:?}\n", judge.output_names());

    let cases: &[(&str, &str)] = &[
        (
            "I prefer the dark theme in every editor.",
            "I prefer the light theme in every editor.",
        ),
        ("I love pizza.", "Pizza is my favourite food."),
        (
            "The deploy region is eu-west-3.",
            "The on-call rotation is weekly.",
        ),
    ];
    for (a, b) in cases {
        let (lab, sc) = judge.classify(a, b)?;
        let same = judge.is_same_fact(a, b)?;
        println!(
            "  [{:>13}]  same_fact={:<5}  c={:.2} e={:.2} n={:.2}",
            lab.as_str(),
            same,
            sc[0],
            sc[1],
            sc[2]
        );
        println!("      A: {a}");
        println!("      B: {b}");
    }

    // The two safety-critical invariants.
    let opposite_is_contra = judge.classify(cases[0].0, cases[0].1)?.0 == NliLabel::Contradiction;
    let opposite_not_merged = !judge.is_same_fact(cases[0].0, cases[0].1)?;
    let paraphrase_is_same = judge.is_same_fact(cases[1].0, cases[1].1)?;

    println!();
    println!(
        "  CRITICAL  opposite preference → contradiction : {}",
        if opposite_is_contra { "PASS" } else { "FAIL" }
    );
    println!(
        "  CRITICAL  opposite preference NOT merged      : {}",
        if opposite_not_merged { "PASS" } else { "FAIL" }
    );
    println!(
        "  CRITICAL  paraphrase → same fact              : {}",
        if paraphrase_is_same { "PASS" } else { "FAIL" }
    );

    if opposite_is_contra && opposite_not_merged && paraphrase_is_same {
        println!("\n✓ NLI judge READY — contradiction-safe, paraphrase-aware.");
        Ok(())
    } else {
        anyhow::bail!("NLI readiness check FAILED — model would be unsafe for paraphrase merges");
    }
}
