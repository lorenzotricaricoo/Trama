# 0005. macOS first, Windows-ready

- **Status:** Accepted
- **Date:** 2026-11-27

## Context

Trama targets macOS and Windows. Building both at once doubles the platform-specific
work (audio capture, permissions, packaging) while the core product is still unproven.
Choosing a macOS-only technology, on the other hand, would force a rewrite later.

## Decision

- The **MVP ships on macOS only**.
- The **stack and architecture are chosen for both platforms** from day one:
  cross-platform core, platform-specific code isolated behind interfaces
  (for example audio capture, credential storage, notifications).
- **Windows** follows after the MVP.
- The tech stack itself is decided in a separate ADR, after the spikes.

## Consequences

- Faster path to a usable MVP.
- Every platform-specific component needs an interface and a macOS implementation;
  the Windows implementation is planned but not built.
- Spikes must check that each chosen technology also works on Windows.

## Alternatives considered

- **Both platforms in the MVP**: more work and risk before the product is validated.
- **macOS-native stack**: rejected, it would require a rewrite for Windows.
