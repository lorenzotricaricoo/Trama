# 0008. Distribution without notarization

- **Status:** Proposed
- **Date:** 2026-11-28
- **Replaces:** the *Packaging* row of [ADR 0007](0007-tech-stack.md)

## Context

[ADR 0007](0007-tech-stack.md) planned a Developer ID signature and notarization, which
need a paid Apple Developer Program membership. There is none, and none is planned
([#39](https://github.com/lorenzotricaricoo/Trama/issues/39)).

Without notarization, macOS quarantines a downloaded app and Gatekeeper blocks its first
launch until the user allows it in **System Settings → Privacy & Security → Open
Anyway**. After that, the app opens normally.

macOS also ties the microphone and system audio permissions to the app's **designated
requirement** ([macOS signing](../development/macos-signing.md)):

- with an **ad hoc** signature the requirement is the hash of the binary, so every new
  version is a different app to macOS and the permissions are asked for again;
- with a **certificate** (an Apple Development certificate from a free Apple account, or
  a self-signed code-signing certificate) the requirement names the certificate, so a
  new version signed by the same certificate keeps the permissions. Neither needs the
  paid program, and neither avoids the Gatekeeper step.

## Decision

- Phase 1 ships as an official **GitHub Release** with a downloadable `.dmg`.
- `Trama.app` and the `.dmg` are signed with the project's **stable signing identity**
  (today the maintainer's Apple Development certificate, see
  [macOS signing](../development/macos-signing.md)), not ad hoc, so updates keep the
  audio permissions. They are not notarized.
- The README and the release notes document the first launch: open the `.dmg`, drag
  Trama to Applications, try to open it, then **Privacy & Security → Open Anyway**.
- Before publishing, the maintainer downloads the `.dmg` from GitHub on a **clean macOS
  account** and runs a real meeting end to end: microphone and system audio capture,
  transcription, summary.
- **Updates**: the maintainer installs one build over another and checks whether macOS
  asks for the audio permissions again.
  - If it does not, Trama includes **Tauri's updater**. The updater verifies updates with
    its own signing key, independent of Apple; the private key lives in the maintainer's
    Keychain and never in the repository.
  - If it does, phase 1 ships without the updater, and users update by downloading the
    new `.dmg` from GitHub Releases.
- **Versions** follow Semantic Versioning from `0.1.0` (phase 1); the release notes are
  written from the Conventional Commits merged since the previous release.

## Consequences

- No yearly fee, and nothing changes in how the app is built: `npm run bundle` already
  signs with the stable identity.
- Every new user sees the Gatekeeper step once; the README must make it easy.
- The signing certificate becomes part of the release process. An Apple Development
  certificate lasts one year: renewing it with the same name keeps the permissions,
  otherwise users are asked once more. A self-signed certificate can last longer and
  needs no Apple account, but has to be created and kept safe by the maintainer; it is
  the fallback if the Apple Development certificate becomes a problem.
- Releases are built and signed on the maintainer's Mac, not in CI, since the
  certificate is not shared with GitHub.
- If a paid membership is ever taken, Developer ID and notarization can replace this
  without changing the app's identifier.

## Alternatives considered

- **Ad hoc signature**: needs no certificate at all, but every update is a new app to
  macOS and asks for the audio permissions again.
- **Building and installing locally only**: no Gatekeeper step, since nothing is
  quarantined, but only for people who can build from source.
- **Homebrew cask**: convenient for developers, but it still installs an unnotarized
  app and adds a channel to maintain; it can come later, on top of GitHub Releases.
- **Paid Apple Developer Program**: removes the Gatekeeper step, but its cost is not
  justified for phase 1.
