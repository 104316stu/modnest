# Mod submissions

One JSON file per mod, named after its id (`my-cool-mod.json`). Start from [`_example.json`](_example.json);
the fields and steps are in [AGENTS.md](../AGENTS.md). Files starting with `_` are ignored.

Check yours before opening a pull request:

```bash
python scripts/check_submission.py submissions/my-cool-mod.json
```

Merged files stay here as the record of who submitted what; the live catalog is `registry/registry.json`.
