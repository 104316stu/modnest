# Adding your mod to ModNest

Anyone can suggest a mod. It shows up for everyone once a maintainer has looked at it and merged it.

**Easiest:** ask your AI coding assistant (Claude Code, Codex, Copilot…) to *"add my mod to ModNest following AGENTS.md in github.com/104316stu/modnest"*. [AGENTS.md](AGENTS.md) has every step and command.

**No AI and no Git?** [Open a "Submit a mod" issue](../../issues/new?template=submit-mod.yml) with a link to your mod's zip and source code, and a maintainer can turn it into a submission.

## How it works

1. You add one file, `submissions/your-mod-id.json`, in a pull request from your own fork/branch. Your zip stays on your own GitHub release.
2. A GitHub check tests the submission automatically: download works, checksum matches, no programs or loaders inside, game and dependencies exist.
3. A maintainer reads your mod's source code and merges it if it's good.
4. On merge, GitHub copies your zip into ModNest's releases and adds it to the catalog. Everyone's ModNest sees it within a few minutes.

To update your mod, open another pull request raising `version` (and the new `download`/`sha256`) in the same file.

## What gets accepted

- Open source (link `sourceCode`), does what it says, no malware, no ads, no data collection without asking.
- Just the mod: no BepInEx/MelonLoader inside, no `.exe` or scripts.
- Not someone else's work without permission.

## For maintainers

- Review the source behind each PR before merging; the automatic check can't tell what a DLL does.
- Merging is the only step needed. `publish-submissions.yml` mirrors the zip and updates `registry/registry.json`; re-run it from the Actions tab if needed.
- Consider protecting `main` (Settings → Branches) so changes only arrive through pull requests.
