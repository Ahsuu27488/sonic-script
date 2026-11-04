import json

from sonic_eval import catalog


def _row(engine, sentence, category="urdu", acc=1.0, error=None, latency=500.0, cost=0.0001):
    return {"engine": engine, "fmt": "opus16", "sentence": sentence, "category": category, "repeat": 0,
            "text": "x", "error": error, "accuracy": acc, "script_ok": True, "english_found": 1,
            "english_total": 1, "latency_ms": latency, "cost_usd": cost, "duration_s": 4.0, "at": 1793194131.0}


def _write(path, rows):
    path.mkdir(parents=True)
    (path / "results.jsonl").write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")


PROVIDERS = {
    "chain": [{"engine": "free-one", "wait_ms": 3000}, {"engine": "paid-one"}],
    "providers": {"g": {"key_env": "G_KEY"}, "q": {"key_env": "Q_KEY"}, "o": {"key_env": "O_KEY"}},
    "engines": [
        {"id": "paid-one", "provider": "o", "refine": {"provider": "q"},
         "catalog": {"label": "Paid", "tier": "paid", "limits": {"text": "no free tier"}}},
        {"id": "free-one", "provider": "g", "catalog": {"label": "Free", "tier": "free"}},
        {"id": "not-offered", "provider": "g"},
    ],
}


def test_catalog_groups_free_first_and_keeps_the_best_row_per_clip(tmp_path):
    _write(tmp_path / "new", [_row("free-one", "a", error="HTTP 429"), _row("free-one", "b", acc=0.5),
                              _row("paid-one", "a"), _row("free-one", "s", category="silence")])
    _write(tmp_path / "old", [_row("free-one", "a", acc=0.9)])
    _write(tmp_path / "app", [_row("app-free-one", "a", acc=0.8, latency=300.0)])

    cat = catalog.build(PROVIDERS, tmp_path, ["new", "old"], ["app"], total=44)

    assert [m["id"] for m in cat["models"]] == ["free-one", "paid-one"], "free first; uncatalogued left out"
    free = cat["models"][0]
    assert free["recommended"] and free["keys"] == ["G_KEY"]
    assert free["fallback"] is None and cat["models"][1]["fallback"] == 1
    r = free["results"]
    # "a" failed in the new run, so the old run's success is used; "s" is silence, scored apart.
    assert (r["clips"], r["of"], r["errors"], r["accuracy"], r["silence_ok"]) == (3, 44, 0, 0.7, "1/1")
    assert r["runs"] == ["new", "old"]
    assert free["app_check"]["accuracy"] == 0.8 and free["app_check"]["p50_ms"] == 300
    paid = cat["models"][1]
    assert paid["keys"] == ["O_KEY", "Q_KEY"] and paid["limits"]["text"] == "no free tier"
    assert "app_check" not in paid
    assert r["usd_per_hour"] == round(3 * 0.0001 / 12 * 3600, 3)


def test_catalog_lists_the_chain_in_its_order_before_more_accurate_models(tmp_path):
    providers = {
        "chain": [{"engine": "main"}, {"engine": "backup"}],
        "providers": {"g": {"key_env": "G_KEY"}},
        "engines": [{"id": i, "provider": "g", "catalog": {"label": i, "tier": "free"}}
                    for i in ("best", "backup", "main", "worst")],
    }
    _write(tmp_path / "run", [_row("best", "a", acc=0.99), _row("backup", "a", acc=0.98),
                              _row("main", "a", acc=0.97), _row("worst", "a", acc=0.5)])

    cat = catalog.build(providers, tmp_path, ["run"], total=44)

    assert [m["id"] for m in cat["models"]] == ["main", "backup", "best", "worst"]
