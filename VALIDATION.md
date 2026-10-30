# Validation — 0.9.0

Validated on Windows x64 with Rust 1.99.1, MSVC 14.44 and Windows SDK 10.0.26100 on 30 October 2026:

- Formatting and all-target Clippy with warnings denied passed.
- The normal suite passed **250 tests**, with **six opt-in tests excluded** and no failures. Five excluded tests need an explicitly supplied real client; one writes a temporary native key-store entry.
- Coverage includes profile validation, independent Wiegand 26/34 vectors, bounded allocation, organisation isolation, exclusive locking, atomic CSV import, schema 1/2 migration with private backups, retained transfers/reservations, clone and card-number rewrite verification, and rejected writes.
- Ten automated GUI tests cover rendering, including empty organisation setup and profile editing, without starting hardware operations. These are offscreen egui tests, not a new manual Windows GUI acceptance run.
- Both release executables built for `x86_64-pc-windows-msvc`; the versioned Windows package includes the mock and synthetic organisation examples.
- The public-source check passed 27 private-path probes using Git's ignore rules. The selected source was checked for private identity/path content. Delivery-root rules also exclude private handoff files, shortcuts and old archives. Only reviewed source and synthetic examples are packaged.

Tests use synthetic profiles, users, fobs and device output. Private deployment logs and company data are excluded from public source and release packages.

No physical fob programming, door-reader acceptance, firmware operation, clean-machine installation or remote GitHub Actions run is claimed for this release. The macOS binary included in older deliveries is unchanged; current Mac build parity must be verified on a Mac runner. Windows binaries are unsigned.
