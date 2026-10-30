"""Build a GitHub-ready source ZIP from files permitted by the repository ignore rules.

No Git history, company data, binaries, local handoff files or validation logs are copied.
Uses an isolated temporary Git directory so the working repository is never staged/changed.
"""
from pathlib import Path
import argparse
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PRIVATE_PROBES = [
    "organisation-register.json", "organisation-profile.json", "settings.json",
    "netaccess-na1.json", "netaccess-na1-before-0.8.1-example.json",
    "organisation-before-0.9.0-example.json", "company-roster.csv", "company-users.pdf",
    "company/users.json", "private/config.yml", "exports/cards.csv", "backups/register.json",
    "src/company-register.json", "tests/company-people.csv", "assets/company-logo.png",
    "examples/customer-profile.json", "validation/session.txt", "AGENTS.md", "NETACCESS.md",
    "INTEGRATION.md", "MACOS-UI.md", "device.log", "secret.key", ".env", "dist/app.exe",
    "scripts/__pycache__/package-source.cpython-312.pyc", "src/private.sqlite",
]
REQUIRED = [
    ".gitignore", ".github/workflows/build.yml", "Cargo.toml", "Cargo.lock", "LICENSE",
    "src/main.rs", "src/netaccess.rs", "tests/organisations.rs", "assets/Info.plist",
    "vendor/eframe/Cargo.toml", "vendor/eframe/data/icon.png",
    "vendor/eframe/LICENSE-MIT", "vendor/eframe/LICENSE-APACHE",
    "examples/synthetic-users.csv", "examples/synthetic-organisation-profile.json",
]

def public_files():
    with tempfile.TemporaryDirectory(prefix="studio-public-source-") as directory:
        subprocess.run(["git", "init", "--quiet", directory], check=True, capture_output=True)
        command = ["git", "--git-dir=" + str(Path(directory) / ".git"), "--work-tree=" + str(ROOT)]
        result = subprocess.run(command + ["check-ignore", "--no-index", "--stdin", "-z"],
            input=("\0".join(PRIVATE_PROBES) + "\0").encode("utf-8"), capture_output=True, cwd=ROOT)
        if result.returncode not in (0, 1):
            raise RuntimeError(result.stderr)
        ignored = set(result.stdout.decode("utf-8").split("\0"))
        missing = set(PRIVATE_PROBES) - ignored
        if missing:
            raise RuntimeError("Private paths are not ignored: " + ", ".join(sorted(missing)))
        result = subprocess.run(command + ["ls-files", "--others", "--exclude-standard", "-z"],
            check=True, capture_output=True, cwd=ROOT)
        files = sorted(p for p in result.stdout.decode("utf-8").split("\0") if p)
        absent = set(REQUIRED) - set(files)
        if absent:
            raise RuntimeError("Required public source is missing: " + ", ".join(sorted(absent)))
        for name in files:
            path = ROOT / name
            if path.is_symlink() or not path.is_file() or not path.resolve().is_relative_to(ROOT):
                raise RuntimeError("Unsupported source path: " + name)
        return files

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="Validate ignore rules and public file inventory only")
    parser.add_argument("--output", type=Path, default=ROOT / "dist" / "ProxmarkStudio-0.9.0-GitHub-source.zip")
    args = parser.parse_args()
    files = public_files()
    if args.check:
        print(f"Privacy ignore checks passed; {len(files)} public source files selected.")
        return
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.output, "w", zipfile.ZIP_DEFLATED) as archive:
        for name in files:
            archive.write(ROOT / name, name)
    with zipfile.ZipFile(args.output) as archive:
        if archive.testzip() is not None:
            raise RuntimeError("Source ZIP integrity check failed")
    print(f"Created {args.output.name}: {len(files)} public files; company-data ignore checks passed.")

if __name__ == "__main__":
    main()
