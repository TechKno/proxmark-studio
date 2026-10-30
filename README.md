# Proxmark Studio 0.9.0

A portable Rust desktop app for using a Proxmark3 reader through an independently installed RRG/Iceman client. Windows x64 and macOS Apple Silicon build scripts are included.

## Download

Get the app from [GitHub Releases](https://github.com/TechKno/proxmark-studio/releases/latest). Choose the **Windows-x64.zip** for Windows or **macOS-arm64.zip** for an Apple Silicon Mac. Extract the whole archive; the Mac app is inside the extracted folder. The separately installed RRG/Iceman client is required for physical hardware.

Windows builds are unsigned. Mac builds are ad-hoc signed, not Developer ID signed or notarised. No Intel Mac build is provided. Release notes identify the checks performed for each build.

## Modes

- **Basic:** guided reading, cloning and programming with destination checks and independent read-back.
- **Organisations:** configure an organisation/site, reader interpretation, facility code and allocation range; manage users, assignments, fob notes, clones, deletion and number changes in a private local register.
- **Advanced:** protocol tools, credentials, card memory, diagnostics and an interactive client console. EV2/LRP/EV3-specific operations remain unavailable.

Organisation mode supports EM410x fobs with Wiegand 26 or Wiegand 34 reader output. The selected interpretation must match the site's reader configuration. Other fob types remain in the existing Basic/Advanced workflows.

## Start

Extract the entire Windows release ZIP and open `proxmark-studio.exe`. Keep `mock-pm3.exe` beside it. Configure the separately installed RRG client and COM port in Device setup, or use the isolated demo.

In **Organisations**, choose **New organisation**, enter the profile and confirm the reader format. Save the private register outside the source repository. Add users or import a CSV based on `examples/synthetic-users.csv`. Use **Open organisation** to switch registers. See [ORGANISATIONS.md](ORGANISATIONS.md).

The app does not connect to an access controller or change door permissions. Controller enrolment, holder updates and revocation remain separate. A clone shares the original credential. A successful programming result requires a fresh read; it does not prove door-reader acceptance.

## Build

Use Rust 1.99+ and the locked dependencies. Windows requires the MSVC C++ build tools and Windows SDK. Put build caches outside synced/source folders.

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Windows: run `scripts/package-windows.ps1` with target `x86_64-pc-windows-msvc`. macOS: run `sh scripts/package-macos.sh` with target `aarch64-apple-darwin`. Preserve the local eframe patch in `vendor/eframe`. Windows binaries are unsigned; the Mac script uses local ad-hoc signing only.

## GitHub and company data

Publish the editable project or the generated GitHub source ZIP, not a machine's delivery folder. The repository uses an allowlist plus recursive ignores for organisation registers/profiles, settings, CSV exports, backups, private directories, logs and local handoff documents. Only explicitly named synthetic examples and test fixtures are allowed.

```text
python scripts/package-source.py --check
python scripts/package-source.py
```

The script uses Git's actual ignore rules, checks representative private paths, and packages only allowed source files. Its temporary Git directory does not modify your repository or index. Do not put real data into the synthetic fixture files. Existing tracked files are not made private by adding ignore rules: review `git ls-files` before publishing an existing repository.

GitHub Actions builds/tests Windows and macOS and validates the private-data ignore rules. Local test results and limits are documented in [VALIDATION.md](VALIDATION.md); no remote CI run is implied.

[Contributing](CONTRIBUTING.md) · [Security and private data](SECURITY.md) · [Changes](CHANGELOG.md)

MIT licensed. Independent of RRG/Iceman and NXP.
