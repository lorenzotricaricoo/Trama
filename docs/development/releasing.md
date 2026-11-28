# Releasing

How a version of Trama is published, as decided in
[ADR 0008](../adr/0008-distribution-without-notarization.md): a GitHub Release with a
`.dmg` signed with the project's stable identity and not notarized. Releases are built
and signed on the maintainer's Mac, never in CI.

## Versions

[Semantic Versioning](https://semver.org/), from `0.1.0` for phase 1. Pre-releases add a
suffix (`0.1.0-alpha.1`, `0.1.0-beta.1`) and are published as GitHub pre-releases. The
tag is the version with a `v` in front (`v0.1.0-alpha.1`).

The version is written in three places, and `npm run bundle` stops if they differ:

| File | Field |
|------|-------|
| `Cargo.toml` (repository root) | `[workspace.package] version` |
| `apps/desktop/src-tauri/tauri.conf.json` | `version` |
| `apps/desktop/package.json` | `version`, plus `package-lock.json` (`npm install` updates it) |

## Steps

1. On a branch from an issue, set the new version in the three files and run `cargo
   check` so `Cargo.lock` follows.
2. Add the version to [CHANGELOG.md](../../CHANGELOG.md), written from the Conventional
   Commits merged since the previous tag (`git log --oneline v<previous>..main`): what a
   user notices, not the internals.
3. Open the PR, let CI pass, merge it.
4. On the merged `main`, build and sign from `apps/desktop`:

   ```sh
   npm ci
   npm run bundle
   ```

   The script prints the version, the signing certificate, the designated requirement
   and the SHA-256 of the `.dmg`. The designated requirement must be the same as the
   previous release's, or macOS will ask for the audio permissions again.
5. Run the [manual checks](#manual-checks) on the built app.
6. Tag and publish, attaching the `.dmg`:

   ```sh
   git tag -a v0.1.0 -m "Trama 0.1.0"
   git push origin v0.1.0
   gh release create v0.1.0 --verify-tag \
     --title "Trama 0.1.0" --notes-file <notes> \
     ../../target/release/bundle/dmg/Trama_0.1.0_aarch64.dmg
   ```

   The notes are the version's section of the changelog, the [install](../../README.md#install)
   steps and the SHA-256. Add `--prerelease` for a pre-release.
7. Download the `.dmg` from the release page and open it once more: the download is
   quarantined, so this is the path users take, Gatekeeper step included.

## Manual checks

On the bundle built in step 4, installed in `/Applications`. The flows are those of the
[information architecture](../design/information-architecture.md#main-flows); what a version
does not have yet is skipped.

**F1 · First launch**

- [ ] Downloaded copy only: the `.dmg`, and then the app if macOS blocks it again, open
      after **Privacy & Security → Open Anyway**.
- [ ] On a new install (another macOS account, or no `settings.json` and no library) the
      onboarding opens; *Not now* and *Skip setup* work, and the next start resumes at
      the step reached. An existing install does not show it.
- [ ] Settings → Transcription: the model downloads, resumes after a pause and is
      reported ready.
- [ ] Settings → Providers: a provider is added, its key is saved, the connection test
      passes, and a model is chosen for the summary.

**F2 · Record a meeting**

- [ ] The first recording asks for the microphone and the system audio; after allowing
      them, both levels move.
- [ ] Notes taken in the recording window end up on the meeting page.
- [ ] After stopping, transcription shows its progress, then the title and the summary
      appear.
- [ ] A meeting of at least 60 minutes records, transcribes and summarizes.
- [ ] The global shortcut (⌥⌘R) starts and stops a recording from another app, and so
      does the menu bar item.
- [ ] Closing the window during a recording leaves Trama in the menu bar and the
      recording goes on; opening it again brings back the window and the Dock icon.
- [ ] Quitting during a recording asks first.

**F3 · Write a note**

- [ ] ⌘N opens a new note; what is typed is still there after quitting and reopening.
- [ ] A Markdown file dropped on the window, or chosen with ⌘O, becomes a note.

**F4 · Review a meeting**

- [ ] The meeting page shows the summary, the key points and the transcript.
- [ ] The player plays both tracks together; a key point or a line jumps to its moment.
- [ ] Editing the summary and renaming the meeting survive a restart.
- [ ] Sources lists meetings and notes; a word said in a meeting is found and opens the
      meeting at that minute; ⌘K finds it too.
- [ ] Home shows the latest meetings, the recent notes and what is being processed.

**Appearance**

- [ ] Changing the accent changes the Dock icon while Trama is open; the menu bar icon
      is monochrome and turns red only while recording.

**Update**

- [ ] Installing the new version over the previous one keeps the library and does not
      ask for the audio permissions again.
