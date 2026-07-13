//! `buttre-test explain <method> <raw> [--learning]` — per-layer diagnosis of
//! why a raw keystroke sequence renders the way it does.
//!
//! Born from the "yes" incident (ADR-0001): every default repro composes
//! WITHOUT the user's learning store, so a machine-specific pref/overlay hit
//! is invisible to `cargo test` and to the data-file harness. `--learning`
//! loads the real `learning.toml` of THIS machine, making "engine is right
//! but my machine is wrong" diagnosable in one command.

use std::sync::{mpsc, Arc, Mutex};

use buttre_core::state::learning::LearningStore;
use buttre_core::KeyboardBuilder;
use buttre_engine::compose::{compose, compose_closed, is_last_event_undo, ComposeOpts, Pref};
use buttre_engine::pipeline::presets;
use buttre_engine::pipeline::validation::{is_attested, is_shape_attested};
use buttre_engine::types::Action;
use colored::Colorize;

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let usage = "usage: buttre-test explain <telex|vni> <raw> [--learning]";
    let method = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!(usage))?;
    let raw_str = args.get(1).cloned().ok_or_else(|| anyhow::anyhow!(usage))?;
    let with_learning = args.iter().any(|a| a == "--learning");
    let raw: Vec<char> = raw_str.chars().collect();

    let cfg = match method {
        "telex" => presets::telex_config(),
        "vni" => presets::vni_config(),
        other => anyhow::bail!("unknown method '{other}' — telex|vni. {usage}"),
    };
    let mut opts = ComposeOpts::from_config(&cfg);

    println!(
        "{} {} ({})",
        "explain".bright_blue().bold(),
        raw_str.bold(),
        method
    );

    // ── Layer 0: learning store ──────────────────────────────────────────────
    let store = with_learning.then(LearningStore::load);
    if let Some(store) = &store {
        let snapshot = store.snapshot_for_method(method);
        let overlay_n = snapshot.user_attested.as_ref().map_or(0, |s| s.len());
        let prefs = store.prefs_snapshot_for_method(method);
        println!(
            "learning.toml: {} overlay syllable(s) đã đạt ngưỡng, {} pref cho method này",
            overlay_n,
            prefs.len()
        );
        match prefs.get(&raw_str.to_lowercase()) {
            Some(p) => {
                let ignored = *p == Pref::Literal && is_last_event_undo(&raw, &opts);
                println!(
                    "  pref hit: '{}' = {:?}{}",
                    raw_str.to_lowercase(),
                    p,
                    if ignored {
                        " — raw dạng undo, pref Literal BỊ BỎ QUA (ADR-0001)"
                            .yellow()
                            .to_string()
                    } else {
                        " — SẼ ÁP DỤNG".red().bold().to_string()
                    }
                );
            }
            None => println!("  pref hit: không"),
        }
        opts.user_attested = snapshot.user_attested.clone();
        opts.raw_prefs = snapshot.raw_prefs.clone();
    } else {
        println!("learning.toml: KHÔNG nạp (thêm --learning để chẩn đoán máy thật)");
    }

    // ── Layer 1: undo shape ──────────────────────────────────────────────────
    println!(
        "undo shape (is_last_event_undo): {}",
        if is_last_event_undo(&raw, &opts) {
            "CÓ"
        } else {
            "không"
        }
    );

    // ── Layer 2: compose, open vs closed ─────────────────────────────────────
    let open = compose(&raw, &opts);
    let closed = compose_closed(&raw, &opts);
    println!(
        "compose (đang gõ):    '{}'  [temp_english={} demoted={} marks={}]",
        open.text.bold(),
        open.temp_english,
        open.demoted,
        open.applied_marks.len()
    );
    println!(
        "compose (chốt từ):    '{}'  [temp_english={} demoted={}]",
        closed.text.bold(),
        closed.temp_english,
        closed.demoted
    );
    println!(
        "attestation của '{}': exact={} shape={}",
        closed.text,
        is_attested(&closed.text),
        is_shape_attested(&closed.text)
    );

    // ── Layer 3: full Keyboard path (per-keystroke screen) ───────────────────
    let mut kb = match method {
        "vni" => KeyboardBuilder::vni()?,
        _ => KeyboardBuilder::telex()?,
    };
    if let Some(store) = store {
        let (tx, _rx) = mpsc::channel();
        kb.set_learning(Arc::new(Mutex::new(store)), tx);
    }
    let mut screen = String::new();
    println!("màn hình theo từng phím:");
    for (i, &ch) in raw.iter().enumerate() {
        for a in kb.process(ch)? {
            apply(&mut screen, &a);
        }
        println!("  {}  '{}'", raw[..=i].iter().collect::<String>(), screen);
    }
    for a in kb.process(' ')? {
        apply(&mut screen, &a);
    }
    println!("sau dấu cách (commit): '{}'", screen.trim_end().bold());
    Ok(())
}

fn apply(screen: &mut String, a: &Action) {
    match a {
        Action::DoNothing | Action::HideCandidates | Action::ShowCandidates { .. } => {}
        Action::Commit(t) | Action::ConfirmComposition(t) => screen.push_str(t),
        Action::Replace {
            backspace_count,
            text,
        } => {
            for _ in 0..*backspace_count {
                screen.pop();
            }
            screen.push_str(text);
        }
        Action::UpdateComposition { .. } => {}
    }
}
