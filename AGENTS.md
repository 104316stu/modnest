# Instructions for AI agents

You're probably here because someone asked you to **add their mod to ModNest**. Do exactly this; it takes a few minutes.
Don't edit `registry/registry.json` or any code. A submission is **one new JSON file** in a pull request; a maintainer reviews it,
and merging publishes it automatically.

## 1. Get the mod ready (in the person's own GitHub repo)

- The mod's source code must be public (a GitHub repo is fine). Reviewers read it before merging.
- Zip **only the mod** (no BepInEx, no MelonLoader, no `.exe`/scripts). What goes in the zip depends on `installType` (step 3).
- Upload the zip as a release asset of the person's repo, e.g. `gh release create v1.0.0 my-mod.zip --repo THEIR_USER/THEIR_REPO`.
  The download link is `https://github.com/THEIR_USER/THEIR_REPO/releases/download/v1.0.0/my-mod.zip`.
- Get its SHA-256: `python -c "import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest())" my-mod.zip`

## 2. Fork and branch

```bash
gh repo fork 104316stu/modnest --clone --remote
cd modnest
git checkout -b mod/MOD-ID
```

## 3. Write `submissions/MOD-ID.json`

Copy `submissions/_example.json`. The file name must be the id. Fields:

| field | required | meaning |
|---|---|---|
| `id` | yes | unique, lowercase `a-z 0-9 -`, 3–50 chars. Never change it later |
| `name`, `author`, `description` | yes | shown in ModNest. `author` is the person's name/handle (only they can update this id later) |
| `version` | yes | like `1.0.0`. Raise it for every update |
| `games` | yes | e.g. `["chained-beasts"]`, `["iron-nest"]`, or any Thunderstore game as `"ts-<community>"` (e.g. `"ts-lethal-company"`) |
| `installType` | yes | `plugin`: zip holds the files for `BepInEx/plugins/<installFolder>/` (e.g. `MyMod.dll`)<br>`cf-pack`: a ChainedForge content pack (zip has `pack.json`)<br>`thunderstore`: a Thunderstore-style zip (`plugins/`, `config/`, `patchers/`)<br>`melon`: MelonLoader (`Mods/`, `UserLibs/`)<br>`folder`: plain files copied into the game's mods folder<br>`modpack`: no zip; `dependencies` lists the mods |
| `installFolder` | for `plugin`/`cf-pack` | folder name, e.g. `MyCoolMod` |
| `dependencies` | if needed | catalog ids it needs, e.g. `["chainedforge"]`. ChainedForge packs need `chainedforge` |
| `download` | not for modpacks | https link to the zip from step 1 |
| `sha256` | not for modpacks | from step 1 |
| `sourceCode` | yes | https link to the source |
| `website`, `icon` | no | https links (icon: a square PNG) |

Current catalog ids (for `dependencies`) are in `registry/registry.json` → `packages[].id`.

## 4. Check it, then open the pull request

```bash
python scripts/check_submission.py submissions/MOD-ID.json   # must end with ✅
git add submissions/MOD-ID.json
git commit -m "Add MOD NAME VERSION"
git push -u origin mod/MOD-ID
gh pr create --repo 104316stu/modnest --title "Add MOD NAME VERSION" --body "What it does, which game, link to source."
```

GitHub runs the same check on the pull request. If it fails, fix the file and push again.

## Updating a mod later

Same steps on a new branch: change `version`, `download` and `sha256` in the same `submissions/MOD-ID.json` (same `id`, same `author`).

## Rules the check enforces

No mod loaders or programs inside the zip, no paths that leave the game folder, zip under 100 MB, the download must match `sha256`,
games and dependencies must exist, and an existing id can only be updated by the same author.
