use anyhow::{Context, Result};
use buttre_core::KeyboardBuilder;
use colored::Colorize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

mod explain;

fn main() -> Result<()> {
    // `buttre-test explain <method> <raw> [--learning]` — one-shot per-layer
    // diagnosis (ADR-0001's "engine is right but my machine is wrong" tool).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("explain") {
        return explain::run(&args[1..]);
    }

    println!("{}", "buttre Integration Test Suite".bright_blue().bold());
    println!("---------------------------");

    // Vietnamese typing tests
    let telex_path = "crates/buttre-test/data/telex.txt";
    let vni_path = "crates/buttre-test/data/vni.txt";

    run_test_file("Telex", telex_path, KeyboardBuilder::telex)?;
    run_test_file("VNI", vni_path, KeyboardBuilder::vni)?;

    // Fallback tests (optional)
    let telex_fallback_path = "crates/buttre-test/data/telex-fallback.txt";
    let vni_fallback_path = "crates/buttre-test/data/vni-fallback.txt";

    if Path::new(telex_fallback_path).exists() {
        run_test_file("Telex Fallback", telex_fallback_path, || {
            KeyboardBuilder::telex()
        })?;
    }

    if Path::new(vni_fallback_path).exists() {
        run_test_file("VNI Fallback", vni_fallback_path, KeyboardBuilder::vni)?;
    }

    println!("\n{}", "All test runs completed.".green().bold());
    Ok(())
}

fn run_test_file<F>(name: &str, path: &str, builder: F) -> Result<()>
where
    F: Fn() -> Result<buttre_core::Keyboard>,
{
    println!("\nTesting {} using {}...", name.bold(), path.italic());

    let file = File::open(path).with_context(|| format!("Failed to open {}", path))?;
    let reader = BufReader::new(file);

    let mut pass_count = 0;
    let mut fail_count = 0;
    let mut errors = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split(", ").collect();
        if parts.len() != 2 {
            continue;
        }

        let typing = parts[0];
        let expected = parts[1];

        let mut keyboard = builder()?;

        for ch in typing.chars() {
            keyboard.process(ch)?;
        }

        let result = keyboard.buffer();
        if result == expected {
            pass_count += 1;
        } else {
            fail_count += 1;
            let expected_hex: Vec<String> = expected
                .chars()
                .map(|c| format!("{:04X}", c as u32))
                .collect();
            let result_hex: Vec<String> = result
                .chars()
                .map(|c| format!("{:04X}", c as u32))
                .collect();
            errors.push(format!(
                "Line {}: typing='{}', expected='{}' ({}), got='{}' ({})",
                index + 1,
                typing,
                expected,
                expected_hex.join(" "),
                result,
                result_hex.join(" ")
            ));
        }
    }

    println!("  Pass: {}", pass_count.to_string().green());
    println!("  Fail: {}", fail_count.to_string().red());

    if !errors.is_empty() {
        println!("\nErrors in {}:", name);
        for err in errors.iter().take(10) {
            println!("  {}", err.red());
        }
        if errors.len() > 10 {
            println!("  ... and {} more errors", errors.len() - 10);
        }
        // return Err(anyhow::anyhow!("Tests failed for {}", name));
    }

    Ok(())
}
