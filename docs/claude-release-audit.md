# Packaging and updater audit

Audit date: 2026-11-06
Scope: Rust portable archive, installer input verification, release splitting/joining,
Rust launcher, and Rust self-updater.

## Fixed

`packaging/archive_rust_portable.py` compared every stage file with
`BUILD-INFO.json`. A Python smoke/runtime run can add `__pycache__`, `.pyc`, or
`.pyo` files after the manifest is written. The installer verifier already
excluded those transient files, but the portable archiver did not, so an
otherwise valid, smoke-tested stage could not be archived.

The archiver now excludes those bytecode paths from both the manifest file-set
comparison and the ZIP payload, matching `build_rust_installer.py`.

The updater now has a Cancel action while downloading/verifying/joining. It
checks cancellation between stream blocks, removes only its own partial file,
and keeps already verified parts for a later retry. The action is disabled
once Setup is being launched because interrupting an installer is unsafe.

The updater now treats a non-zero Setup process exit code as an installation
failure instead of always showing success. It keeps the verified installer on
failure so the user can retry it.

Evidence:

- `releases/VietKaraStudio-Rust-Portable-x64-build4` contains runtime
  `__pycache__` directories and `.pyc` files.
- `build_rust_installer.py --verify-only` passed against that stage (28,703
  verified payload files; transient bytecode excluded).
- A disposable fixture stage with an unmanifested `.pyc` was archived; the
  payload and checksum verified, and the `.pyc` was absent from the ZIP.

## Verified locally

- `tests/test_rust_update_helper.ps1` passed. It compiles the helper and checks
  valid and invalid downloads, interrupted streams, part joining, missing
  parts, cancellation cleanup, Setup exit-code handling, and cleanup of
  partial files created by the helper.
- `packaging/build_rust_installer.py releases/<stage> --verify-only` passed for
  the existing build4 stage.
- Static review found the downloader validates HTTPS/allowed GitHub hosts,
  each part's expected name, size and SHA-256, then verifies the complete
  installer before launching Setup. Publish uses exclusive file creation, so
  it does not overwrite an existing destination.

## Release blockers and remaining checks

1. **Updater network E2E is blocked until a Rust release is published.** The
   updater requires stable-release assets named
   `VietKaraStudio-Rust-<version>-Setup-x64.exe.001...`, plus `downloads.json`
   containing the matching part manifest. At audit time, the public latest
   release was v1.1.7 and its assets were the Nuitka patch ZIP and SHA-256
   sidecar; no Rust Setup parts or Rust manifest were available. The updater
   therefore cannot complete a real update yet. After publishing a Rust
   release, test from an installed older Rust Setup through download,
   verification, in-place install, app-data preservation, and relaunch.

2. **The installer and updater executable are not code-signed.** SHA-256 checks
   catch damaged or mismatched artifacts, but the manifest is downloaded from
   the same GitHub release account and is not a publisher signature. A
   production release still needs signing and clean-machine SmartScreen QA.

3. **Installer runtime coverage remains scoped to the existing Windows smoke
   records.** This audit did not reinstall or uninstall build4; the prior
   `docs/windows-plan.md` records install/uninstall smoke passes, while a clean
   Windows VM and update/rollback E2E remain outstanding.

The current Rust release flow is not ready for an end-to-end updater claim
until blocker 1 is resolved. No other confirmed defect was found in the
reviewed updater path.
