# Contributing

Use the locked Rust dependencies and keep compiler output outside the source tree. Preserve the local eframe accessibility patch. Run formatting, all-target Clippy, the normal test suite and `python scripts/package-source.py --check` before proposing a change.

Use invented users, card identifiers and device output in tests. Never add a real company roster, register, controller export, physical card dump, key, client settings, machine-specific shortcut or private validation log. The named examples and fixtures are explicit ignore-rule exceptions and must remain synthetic.

Hardware actions are distinct from software validation. Report which tests ran and which require opt-in. Do not enable unsupported protocols or flash a reader to make tests pass. Keep current matching firmware checks, destination review and independent read-back.

For GitHub, create a repository from this project or extract the generated source ZIP, then review `git status --short` and `git ls-files` before your first push. `.gitignore` does not remove previously tracked data or historical commits. No credentials or company files are needed by CI.
