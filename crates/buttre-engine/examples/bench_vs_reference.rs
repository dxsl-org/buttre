//! Native cross-engine benchmark comparator: buttre vs reference engines.
//!
//! Measures latency (mean, P50, P95, P99 in nanoseconds), throughput (M keystrokes/sec),
//! and correctness against linguistic golden datasets.

use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use buttre_engine::compose::{compose, ComposeOpts};
use buttre_engine::pipeline::{presets, PipelineExecutor};

// Fallback built-in test dataset if external data files aren't found
const BUILTIN_TELEX_CASES: &[(&str, &str)] = &[
    ("nguwowif", "người"),
    ("tuongwf", "tường"),
    ("dduwowcj", "được"),
    ("thuongwf", "thường"),
    ("bieecs", "biếc"),
    ("buoongf", "buồng"),
    ("nghiengs", "nghiếng"),
    ("khuyaa", "khuyâ"),
    ("hoawj", "hoặc"),
    ("quyeenf", "quyền"),
    ("chuyeern", "chuyển"),
    ("vieetj", "việt"),
    ("namf", "nàm"),
    ("aa", "â"),
    ("awf", "ằ"),
    ("text", "text"), // English auto-restore / passthrough
    ("expect", "expect"),
    ("window", "window"),
    ("weird", "weird"),
];

const BUILTIN_VNI_CASES: &[(&str, &str)] = &[
    ("ngu7o72i", "người"),
    ("tuo7ng2", "tường"),
    ("d9u7o7c5", "được"),
    ("thu7o7ng2", "thường"),
    ("bie6c1", "biếc"),
    ("buo6ng2", "buồng"),
    ("nghie6ng1", "nghiếng"),
    ("quye6n2", "quyền"),
    ("chuye6n3", "chuyển"),
    ("vie6t5", "việt"),
    ("a6", "â"),
    ("a82", "ằ"),
];

#[derive(Debug, Clone, serde::Serialize)]
struct BenchResult {
    engine_name: String,
    method: String,
    total_keystrokes: usize,
    total_time_us: u128,
    mean_ns_per_key: f64,
    p50_ns: u64,
    p95_ns: u64,
    p99_ns: u64,
    throughput_mkeys_sec: f64,
    accuracy_percent: f64,
    pass_count: usize,
    total_words: usize,
}

fn char_to_gonhanh_key(c: char) -> u16 {
    match c.to_ascii_lowercase() {
        'a' => 0,
        's' => 1,
        'd' => 2,
        'f' => 3,
        'h' => 4,
        'g' => 5,
        'z' => 6,
        'x' => 7,
        'c' => 8,
        'v' => 9,
        'b' => 11,
        'q' => 12,
        'w' => 13,
        'e' => 14,
        'r' => 15,
        'y' => 16,
        't' => 17,
        'o' => 31,
        'u' => 32,
        'i' => 34,
        'p' => 35,
        'l' => 37,
        'j' => 38,
        'k' => 40,
        'n' => 45,
        'm' => 46,
        '1' => 18,
        '2' => 19,
        '3' => 20,
        '4' => 21,
        '5' => 23,
        '6' => 22,
        '7' => 26,
        '8' => 28,
        '9' => 25,
        '0' => 29,
        ' ' => 49,
        '.' => 47,
        ',' => 43,
        '/' => 44,
        ';' => 41,
        '\'' => 39,
        '[' => 33,
        ']' => 30,
        '\\' => 42,
        '-' => 27,
        '=' => 24,
        '\x1b' => 53,
        _ => 255,
    }
}

fn run_gonhanh_word(engine: &mut gonhanh_core::engine::Engine, input: &str) -> String {
    let mut screen = String::new();
    for c in input.chars() {
        let key = char_to_gonhanh_key(c);
        if key == 255 {
            screen.push(c);
            continue;
        }
        let is_caps = c.is_uppercase();
        let r = engine.on_key(key, is_caps, false);
        if r.action == 1 {
            for _ in 0..r.backspace {
                screen.pop();
            }
            for i in 0..r.count as usize {
                if let Some(ch) = char::from_u32(r.chars[i]) {
                    screen.push(ch);
                }
            }
        } else {
            screen.push(c);
        }
    }
    screen
}

fn load_dataset(path: &str, builtin: &[(&str, &str)]) -> Vec<(String, String)> {
    if Path::new(path).exists() {
        if let Ok(file) = File::open(path) {
            let reader = BufReader::new(file);
            let mut result = Vec::new();
            for line in reader.lines().map_while(Result::ok) {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let parts: Vec<&str> = trimmed.split(", ").collect();
                if parts.len() == 2 {
                    result.push((parts[0].to_string(), parts[1].to_string()));
                }
            }
            if !result.is_empty() {
                return result;
            }
        }
    }
    builtin
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let json_mode = args.iter().any(|arg| arg == "--json");

    let telex_data = load_dataset("crates/buttre-test/data/telex.txt", BUILTIN_TELEX_CASES);
    let vni_data = load_dataset("crates/buttre-test/data/vni.txt", BUILTIN_VNI_CASES);

    let mut results = Vec::new();

    // 1. buttre-engine::compose (Telex)
    {
        let config = presets::telex_config();
        let opts = ComposeOpts::from_config(&config);
        let mut pass = 0;
        let total_words = telex_data.len();
        let mut key_times = Vec::with_capacity(total_words * 10);

        // Accuracy check & warmup
        for (input, expected) in &telex_data {
            let raw: Vec<char> = input.chars().collect();
            let out = compose(&raw, &opts);
            if out.text == *expected {
                pass += 1;
            }
        }

        let iterations = (100_000 / total_words.max(1)).max(10);
        let mut total_keys = 0;
        let start = Instant::now();

        for _ in 0..iterations {
            for (input, _) in &telex_data {
                let raw: Vec<char> = input.chars().collect();
                total_keys += raw.len();
                let t0 = Instant::now();
                let _ = compose(&raw, &opts);
                key_times.push((t0.elapsed().as_nanos() as u64) / raw.len().max(1) as u64);
            }
        }
        let total_time_us = start.elapsed().as_micros();
        key_times.sort_unstable();

        results.push(BenchResult {
            engine_name: "buttre::compose (stateless)".to_string(),
            method: "Telex".to_string(),
            total_keystrokes: total_keys,
            total_time_us,
            mean_ns_per_key: (total_time_us as f64 * 1000.0) / total_keys as f64,
            p50_ns: key_times[key_times.len() * 50 / 100],
            p95_ns: key_times[key_times.len() * 95 / 100],
            p99_ns: key_times[key_times.len() * 99 / 100],
            throughput_mkeys_sec: (total_keys as f64) / (total_time_us as f64),
            accuracy_percent: (pass as f64 / total_words as f64) * 100.0,
            pass_count: pass,
            total_words,
        });
    }

    // 2. buttre-engine::PipelineExecutor (Telex)
    //
    // Steady-state methodology: a real IME constructs the executor ONCE per
    // session and resets between words — constructing config + executor per
    // word inside the timed window inflated the mean ~2.3x (measured).
    {
        let mut pass = 0;
        let total_words = telex_data.len();
        let mut key_times = Vec::with_capacity(total_words * 10);
        let mut executor = PipelineExecutor::new(presets::telex_config());

        for (input, expected) in &telex_data {
            executor.reset();
            for ch in input.chars() {
                let _ = executor.process(ch);
            }
            if executor.syllable() == *expected {
                pass += 1;
            }
        }

        let iterations = (100_000 / total_words.max(1)).max(10);
        let mut total_keys = 0;
        let start = Instant::now();

        for _ in 0..iterations {
            for (input, _) in &telex_data {
                executor.reset();
                for ch in input.chars() {
                    total_keys += 1;
                    let t0 = Instant::now();
                    let _ = executor.process(ch);
                    key_times.push(t0.elapsed().as_nanos() as u64);
                }
            }
        }
        let total_time_us = start.elapsed().as_micros();
        key_times.sort_unstable();

        results.push(BenchResult {
            engine_name: "buttre::PipelineExecutor (stateful)".to_string(),
            method: "Telex".to_string(),
            total_keystrokes: total_keys,
            total_time_us,
            mean_ns_per_key: (total_time_us as f64 * 1000.0) / total_keys as f64,
            p50_ns: key_times[key_times.len() * 50 / 100],
            p95_ns: key_times[key_times.len() * 95 / 100],
            p99_ns: key_times[key_times.len() * 99 / 100],
            throughput_mkeys_sec: (total_keys as f64) / (total_time_us as f64),
            accuracy_percent: (pass as f64 / total_words as f64) * 100.0,
            pass_count: pass,
            total_words,
        });
    }

    // 3. gonhanh-core::Engine (Telex)
    //
    // Same steady-state methodology as the executor above: one engine per
    // session, `clear()` between words.
    {
        let mut pass = 0;
        let total_words = telex_data.len();
        let mut key_times = Vec::with_capacity(total_words * 10);
        let mut engine = gonhanh_core::engine::Engine::new();
        engine.set_method(0); // Telex

        for (input, expected) in &telex_data {
            engine.clear();
            let out = run_gonhanh_word(&mut engine, input);
            if out == *expected {
                pass += 1;
            }
        }

        let iterations = (100_000 / total_words.max(1)).max(10);
        let mut total_keys = 0;
        let start = Instant::now();

        for _ in 0..iterations {
            for (input, _) in &telex_data {
                engine.clear();
                for ch in input.chars() {
                    let key = char_to_gonhanh_key(ch);
                    if key != 255 {
                        total_keys += 1;
                        let t0 = Instant::now();
                        let _ = engine.on_key(key, ch.is_uppercase(), false);
                        key_times.push(t0.elapsed().as_nanos() as u64);
                    }
                }
            }
        }
        let total_time_us = start.elapsed().as_micros();
        key_times.sort_unstable();

        results.push(BenchResult {
            engine_name: "gonhanh::Engine (.reference)".to_string(),
            method: "Telex".to_string(),
            total_keystrokes: total_keys,
            total_time_us,
            mean_ns_per_key: (total_time_us as f64 * 1000.0) / total_keys as f64,
            p50_ns: key_times[key_times.len() * 50 / 100],
            p95_ns: key_times[key_times.len() * 95 / 100],
            p99_ns: key_times[key_times.len() * 99 / 100],
            throughput_mkeys_sec: (total_keys as f64) / (total_time_us as f64),
            accuracy_percent: (pass as f64 / total_words as f64) * 100.0,
            pass_count: pass,
            total_words,
        });
    }

    // Output formatting
    if json_mode {
        let json = serde_json::to_string_pretty(&results).unwrap();
        println!("{}", json);
    } else {
        println!("\n==========================================================================================================");
        println!("                         VIETNAMESE INPUT ENGINE NATIVE BENCHMARK (REAL DATA)                             ");
        println!("==========================================================================================================");
        println!(
            "{:<35} | {:<7} | {:<10} | {:<10} | {:<8} | {:<12} | {:<10}",
            "Engine", "Method", "Mean (ns)", "P95 (ns)", "MKey/s", "Accuracy", "Keystrokes"
        );
        println!("----------------------------------------------------------------------------------------------------------");
        for r in &results {
            println!(
                "{:<35} | {:<7} | {:<10.2} | {:<10} | {:<8.2} | {:>5}/{} ({:>5.1}%) | {:<10}",
                r.engine_name,
                r.method,
                r.mean_ns_per_key,
                r.p95_ns,
                r.throughput_mkeys_sec,
                r.pass_count,
                r.total_words,
                r.accuracy_percent,
                r.total_keystrokes
            );
        }
        println!("==========================================================================================================\n");
    }
}
