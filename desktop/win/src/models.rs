//! `--models`: the model catalog (`spec/models.json`, D-017) in the console, free models first, until the
//! settings page shows it (M2). The file is written by `python -m sonic_eval catalog` and read at build time.

use serde_json::Value;

const MODELS: &str = include_str!("../../../spec/models.json");

pub fn print() -> Result<(), String> {
    let catalog: Value =
        serde_json::from_str(MODELS).map_err(|e| format!("spec/models.json: {e}"))?;
    let models = catalog["models"].as_array().cloned().unwrap_or_default();
    for (tier, title) in [
        ("free", "FREE (your own free key; daily limits)"),
        ("paid", "PAID (billed to your key)"),
    ] {
        println!("\n{title}");
        for m in models.iter().filter(|m| m["tier"] == tier) {
            print_model(m);
        }
    }
    println!(
        "\nMeasured on {} with the owner's 44 test recordings (runs: {}). Use one with: sonicscript --engine <id>",
        catalog["generated"].as_str().unwrap_or("?"),
        join(&catalog["runs"])
    );
    Ok(())
}

fn print_model(m: &Value) {
    let star = if m["recommended"] == true {
        "  ★ default".to_string()
    } else if let Some(n) = m["fallback"].as_u64() {
        format!("  ↺ fallback {n}")
    } else {
        String::new()
    };
    println!(
        "\n  {}  [{}]{star}",
        m["label"].as_str().unwrap_or("?"),
        m["id"].as_str().unwrap_or("?")
    );
    let r = &m["results"];
    if r.is_object() {
        println!(
            "    accuracy {}  ·  English words kept {}  ·  Roman only {}  ·  silence {}",
            pct(&r["accuracy"]),
            pct(&r["english_kept"]),
            pct(&r["roman_only"]),
            r["silence_ok"].as_str().unwrap_or("-")
        );
        let (clips, of) = (
            r["clips"].as_u64().unwrap_or(0),
            r["of"].as_u64().unwrap_or(0),
        );
        let tested = if clips < of {
            format!("only {clips} of {of} clips tested so far")
        } else {
            format!("{} errors in {clips} clips", r["errors"])
        };
        let price = match r["usd_per_hour"].as_f64() {
            Some(usd) if usd > 0.0 => format!("paid price ${usd:.2} per hour of speech"),
            _ => "paid price not checked".into(),
        };
        println!(
            "    wait: typical {:.1} s, slowest 5% {:.1} s  ·  {tested}  ·  {price}",
            secs(&r["p50_ms"]),
            secs(&r["p95_ms"]),
        );
        let app = &m["app_check"];
        if app.is_object() {
            println!(
                "    in the app (with its cleanup): accuracy {}  ·  Roman only {}  ·  typical {:.1} s, slowest 5% {:.1} s",
                pct(&app["accuracy"]),
                pct(&app["roman_only"]),
                secs(&app["p50_ms"]),
                secs(&app["p95_ms"])
            );
        }
    } else {
        println!("    not benchmarked yet");
    }
    if let Some(text) = m["limits"]["text"].as_str() {
        println!("    limits: {text}");
    }
    println!("    keys: {}", join(&m["keys"]));
    if let Some(notes) = m["notes"].as_str().filter(|n| !n.is_empty()) {
        println!("    {notes}");
    }
}

fn pct(v: &Value) -> String {
    v.as_f64()
        .map_or("-".into(), |x| format!("{:.1}%", x * 100.0))
}

fn secs(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0) / 1000.0
}

fn join(v: &Value) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonicscript_core::spec;

    #[test]
    fn every_catalog_model_is_one_the_app_can_run() {
        let catalog: Value = serde_json::from_str(MODELS).unwrap();
        for m in catalog["models"].as_array().unwrap() {
            let id = m["id"].as_str().unwrap();
            assert!(
                spec::engine(id).is_some(),
                "{id} is in the catalog but not runnable"
            );
            assert!(m["tier"] == "free" || m["tier"] == "paid", "{id}");
        }
    }
}
