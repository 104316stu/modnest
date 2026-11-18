#!/usr/bin/env python3
"""
Checks mod submissions (submissions/*.json) and, after a maintainer merges them, publishes them.

  python scripts/check_submission.py                 check every submission
  python scripts/check_submission.py submissions/x.json   check one
  python scripts/check_submission.py --publish        (GitHub Actions, after merge) mirror the zip
                                                      into this repo's releases and add it to the catalog
  python scripts/check_submission.py --publish --dry-run   show what --publish would do

Standard library only, so any computer (or AI agent) with Python 3.9+ can run it.
"""
import hashlib, io, json, os, re, subprocess, sys, urllib.request, zipfile
from pathlib import PurePosixPath

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REGISTRY = os.path.join(ROOT, "registry", "registry.json")
SUBMISSIONS = os.path.join(ROOT, "submissions")
REPO = os.environ.get("GITHUB_REPOSITORY", "104316stu/modnest")

MAX_BYTES = 100 * 1024 * 1024
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{1,48}[a-z0-9]$")
VERSION_RE = re.compile(r"^\d+(\.\d+){1,3}([-+][0-9A-Za-z.-]+)?$")
INSTALL_TYPES = {"plugin", "cf-pack", "thunderstore", "melon", "folder", "modpack"}
BLOCKED_EXT = {".exe", ".bat", ".cmd", ".ps1", ".vbs", ".scr", ".msi", ".com", ".jar", ".sh", ".lnk", ".reg"}
REQUIRED = ["id", "name", "author", "description", "version", "games", "installType", "sourceCode"]


def load_json(path):
    with open(path, encoding="utf-8-sig") as f:
        return json.load(f)


def known_games(reg):
    """Game ids a submission may target: the catalog's games, or any Thunderstore game as ts-<community>."""
    return {g["id"] for g in reg.get("games", [])}


def download(url):
    req = urllib.request.Request(url, headers={"User-Agent": "ModNest-submission-check"})
    with urllib.request.urlopen(req, timeout=60) as r:
        data = r.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError(f"download is over {MAX_BYTES // 1048576} MB")
    return data


def check_zip(sub, data, problems, notes):
    try:
        z = zipfile.ZipFile(io.BytesIO(data))
    except zipfile.BadZipFile:
        problems.append("the download is not a zip file")
        return
    names = [n.replace("\\", "/") for n in z.namelist()]
    files = [n for n in names if not n.endswith("/")]
    if not files:
        problems.append("the zip is empty")
        return
    for n in files:
        p = PurePosixPath(n)
        if p.is_absolute() or ".." in p.parts or re.match(r"^[A-Za-z]:", n):
            problems.append(f"unsafe path in zip: {n}")
        if p.suffix.lower() in BLOCKED_EXT:
            problems.append(f"programs/scripts aren't allowed in mods: {n}")
    t = sub.get("installType")
    lower = [f.lower() for f in files]
    if t in ("plugin", "thunderstore", "melon") and not any(f.endswith(".dll") for f in lower):
        problems.append(f"installType '{t}' expects at least one .dll in the zip")
    if t == "cf-pack" and not any(f.endswith("pack.json") for f in lower):
        problems.append("installType 'cf-pack' needs a pack.json in the zip")
    if any(f.split("/")[0] in ("winhttp.dll", "version.dll", "doorstop_config.ini") for f in lower):
        problems.append("the zip contains a mod loader (winhttp.dll / version.dll / doorstop); submit just the mod")
    dlls = [f for f in files if f.lower().endswith(".dll")]
    notes.append(f"zip: {len(files)} files, {len(data) // 1024} KB" + (f", DLLs: {', '.join(dlls[:8])}" if dlls else ""))


def check(path, reg, all_subs):
    problems, notes = [], []
    try:
        sub = load_json(path)
    except Exception as e:
        return [f"not valid JSON: {e}"], notes, None
    fname = os.path.splitext(os.path.basename(path))[0]

    for k in REQUIRED:
        if not sub.get(k):
            problems.append(f"missing \"{k}\"")
    sid = str(sub.get("id", ""))
    if sid and not ID_RE.match(sid):
        problems.append("\"id\" must be 3-50 characters of a-z, 0-9 and dashes")
    if sid and sid != fname:
        problems.append(f"file name must match the id: submissions/{sid}.json")
    if sub.get("version") and not VERSION_RE.match(str(sub["version"])):
        problems.append("\"version\" must look like 1.0.0")
    t = sub.get("installType")
    if t and t not in INSTALL_TYPES:
        problems.append(f"\"installType\" must be one of {sorted(INSTALL_TYPES)}")
    for k in ("sourceCode", "website", "icon", "download"):
        v = sub.get(k)
        if v and not str(v).startswith("https://"):
            problems.append(f"\"{k}\" must be an https:// link")

    games = sub.get("games") or []
    if not isinstance(games, list) or not games:
        problems.append("\"games\" must be a list like [\"chained-beasts\"]")
    else:
        known = known_games(reg)
        for g in games:
            if g not in known and not re.match(r"^ts-[a-z0-9-]+$", str(g)):
                problems.append(f"unknown game \"{g}\" (use one of {sorted(known)} or ts-<thunderstore-community>)")

    catalog_ids = {p["id"] for p in reg.get("packages", [])}
    sub_ids = {s.get("id") for s in all_subs}
    for d in sub.get("dependencies") or []:
        if d not in catalog_ids and d not in sub_ids:
            problems.append(f"dependency \"{d}\" isn't in the catalog or another submission")

    existing = next((p for p in reg.get("packages", []) if p["id"] == sid), None)
    if existing:
        if existing.get("installType") == "root":
            problems.append("mod loaders can't be replaced through submissions")
        if (existing.get("author") or "").lower() != str(sub.get("author", "")).lower():
            problems.append(f"\"{sid}\" already exists by {existing.get('author')}; pick another id (only its author can update it)")
        elif existing.get("version") == sub.get("version"):
            notes.append("already published at this version (nothing to do)")
        else:
            notes.append(f"update: {existing.get('version')} -> {sub.get('version')}")

    if t == "modpack":
        if sub.get("download"):
            problems.append("modpacks have no download; list the mods in \"dependencies\"")
        if not sub.get("dependencies"):
            problems.append("a modpack needs \"dependencies\" (the mods it bundles)")
    elif t:
        if not sub.get("download"):
            problems.append("missing \"download\" (an https link to the mod's zip)")
        if not re.match(r"^[0-9a-f]{64}$", str(sub.get("sha256", ""))):
            problems.append("missing \"sha256\" of the zip (64 hex characters)")
        if sub.get("download") and not problems:
            try:
                data = download(sub["download"])
                got = hashlib.sha256(data).hexdigest()
                if got != sub.get("sha256", "").lower():
                    problems.append(f"sha256 doesn't match the download (download is {got})")
                check_zip(sub, data, problems, notes)
            except Exception as e:
                problems.append(f"couldn't download the zip: {e}")
    return problems, notes, sub


def publish(sub, reg, dry_run):
    """Mirror the zip into this repo's releases and add/replace the catalog entry."""
    sid, ver = sub["id"], sub["version"]
    entry = {k: v for k, v in sub.items() if k not in ("sourceCode", "submittedBy", "notes")}
    entry["website"] = sub.get("website") or sub.get("sourceCode")
    entry.setdefault("dependencies", [])
    entry.setdefault("category", "")
    if sub["installType"] != "modpack":
        tag = f"{sid}-{ver}"
        asset = f"{tag}.zip"
        entry["download"] = f"https://github.com/{REPO}/releases/download/{tag}/{asset}"
        if dry_run:
            print(f"  would create release {tag} with {asset} and point the catalog at it")
        else:
            data = download(sub["download"])
            if hashlib.sha256(data).hexdigest() != sub["sha256"].lower():
                raise SystemExit(f"{sid}: download changed since review (sha256 mismatch); not publishing")
            tmp = os.path.join(os.environ.get("RUNNER_TEMP", "/tmp"), asset)
            with open(tmp, "wb") as f:
                f.write(data)
            notes = f"{sub['name']} {ver} by {sub['author']} (community submission). Source: {sub.get('sourceCode')}"
            subprocess.run(["gh", "release", "create", tag, tmp, "--repo", REPO, "--title", f"{sub['name']} {ver}", "--notes", notes, "--latest=false"], check=True)
    pk = reg.setdefault("packages", [])
    for i, p in enumerate(pk):
        if p["id"] == sid:
            pk[i] = entry
            break
    else:
        pk.append(entry)
    print(f"  catalog: {sid} {ver}")


def main():
    # Windows consoles default to an old code page that can't print ✅/❌.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except Exception:
            pass
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    do_publish = "--publish" in sys.argv
    dry_run = "--dry-run" in sys.argv
    reg = load_json(REGISTRY)
    paths = args or sorted(os.path.join(SUBMISSIONS, f) for f in os.listdir(SUBMISSIONS) if f.endswith(".json") and not f.startswith("_"))
    all_subs = []
    for p in paths:
        try:
            all_subs.append(load_json(p))
        except Exception:
            pass

    summary = ["## Mod submission check", ""]
    failed = False
    to_publish = []
    for p in paths:
        problems, notes, sub = check(p, reg, all_subs)
        name = os.path.relpath(p, ROOT).replace("\\", "/")
        if problems:
            failed = True
            summary.append(f"### ❌ `{name}`")
            summary += [f"- {x}" for x in problems]
        else:
            summary.append(f"### ✅ `{name}`: {sub.get('name')} {sub.get('version')} for {', '.join(sub.get('games', []))}")
            existing = next((x for x in reg.get("packages", []) if x["id"] == sub["id"]), None)
            if not existing or existing.get("version") != sub.get("version"):
                to_publish.append(sub)
        summary += [f"- {x}" for x in notes]
        summary.append("")
    if not paths:
        summary.append("No submissions found.")
    summary.append("A maintainer still needs to review the mod (read its source) before merging." if not failed else "Fix the ❌ items above, then push again.")
    text = "\n".join(summary)
    print(text)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as f:
            f.write(text + "\n")

    if do_publish:
        if failed:
            raise SystemExit("Not publishing: some submissions have problems.")
        if not to_publish:
            print("Nothing new to publish.")
            return
        print("\nPublishing:")
        for sub in to_publish:
            publish(sub, reg, dry_run)
        if not dry_run:
            import datetime
            reg["updated"] = datetime.date.today().isoformat()
            with open(REGISTRY, "w", encoding="utf-8", newline="\n") as f:
                json.dump(reg, f, indent=4, ensure_ascii=False)
                f.write("\n")
    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()
