# macOS signing and bundle

How Trama is signed on macOS and how to build the app bundle. Configured in
[#22](https://github.com/lorenzotricaricoo/Trama/issues/22).

## Why a stable identity

macOS ties the microphone and system audio permissions to the app's **designated
requirement**: its bundle identifier plus the certificate that signed it. With a stable
identity a new build keeps the permissions; with an ad-hoc signature the requirement is
the hash of the binary, so macOS asks again after every build (spike
[#11](https://github.com/lorenzotricaricoo/Trama/issues/11)).

Trama is signed with an **Apple Development** certificate, which a free Apple account can
create in Xcode. A self-signed code-signing certificate works the same way for
permissions. There is no Developer ID certificate and no notarization: that needs a paid
Apple Developer Program membership (see
[#39](https://github.com/lorenzotricaricoo/Trama/issues/39) for distribution).

## Set up once

1. Create the certificate: Xcode → Settings → Accounts → your Apple ID → Manage
   Certificates → **+** → Apple Development.
2. Find its full name:

   ```sh
   security find-identity -v -p codesigning
   ```

3. Tell the build which one to use, in either way (never commit it):
   - write the name in `.signing-identity` at the repository root (gitignored), or
   - set `TRAMA_SIGNING_IDENTITY` in your shell.

   ```sh
   echo 'Apple Development: Jane Doe (ABCDE12345)' > .signing-identity
   ```

## Build

From `apps/desktop`:

```sh
npm run bundle              # release: target/release/bundle/{macos,dmg}
npm run bundle -- --debug   # debug build "Trama Dev" with the DEV icon, same signature
```

The script signs `Trama.app` and the `.dmg`, verifies the signature and prints the
designated requirement. Two builds signed by the same certificate print the same
requirement, so permissions granted once stay granted.

`npm run tauri dev` runs an unbundled binary: macOS attributes its permissions to the
terminal, not to Trama. Use a bundle to test anything that needs a permission.

## What the bundle declares

| File | Content |
|------|---------|
| `src-tauri/Info.plist` | Interface languages, permission prompts for microphone (`NSMicrophoneUsageDescription`) and system audio (`NSAudioCaptureUsageDescription`) |
| `src-tauri/macos/<lang>.lproj/InfoPlist.strings` | The permission prompts translated; a test checks that every language has them |
| `src-tauri/Entitlements.plist` | `com.apple.security.device.audio-input`, required for the microphone under the hardened runtime |
| `src-tauri/tauri.conf.json` | Hardened runtime on, macOS 14.2 or later (Core Audio process tap) |

## Renewing the certificate

An Apple Development certificate lasts one year (`security find-certificate -c "<name>" -p
| openssl x509 -noout -enddate` shows the date). Before it expires, create a new one in
Xcode and update `.signing-identity`. The designated requirement checks the certificate
name: if the renewed certificate keeps the same name the permissions stay, otherwise macOS
asks for them once more.

## Gatekeeper

A bundle built and copied locally runs without warnings. A copy downloaded from the
internet is quarantined, and Gatekeeper rejects it because it is not notarized
(`spctl --assess` reports `rejected`): the user has to allow it in System Settings →
Privacy & Security. How Trama is distributed despite this is decided in
[ADR 0008](../adr/0008-distribution-without-notarization.md); how a release is built and
published is in [Releasing](releasing.md).
