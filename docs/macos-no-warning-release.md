# macOS No-Warning Release Setup

macOS does not provide a reliable no-warning path for unsigned downloaded binaries. If a downloaded build says Apple could not verify `kaspa-miner` is free of malware, that build was not notarized by Apple or the user is launching a raw non-notarized binary instead of the notarized installer package. For broad public distribution, ship the notarized installer package produced by the release workflow.

Required Apple account items:

- Active Apple Developer Program membership.
- Developer ID Application certificate.
- Developer ID Installer certificate.
- App-specific password for the Apple ID used by notarization.
- Apple Team ID.

Apple references:

- https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web

Required GitHub Actions secrets:

```text
APPLE_CERTIFICATE_P12_BASE64
APPLE_CERTIFICATE_PASSWORD
APPLE_TEAM_ID
APPLE_ID
APPLE_APP_PASSWORD
APPLE_APP_SIGN_IDENTITY
APPLE_INSTALLER_SIGN_IDENTITY
```

The `APPLE_CERTIFICATE_P12_BASE64` secret should contain a base64-encoded `.p12` export that includes both Developer ID certificates and private keys.

Example local encoding command:

```sh
base64 -i apple-signing-certificates.p12 | pbcopy
```

Expected identity names look like:

```text
Developer ID Application: Your Name (TEAMID)
Developer ID Installer: Your Name (TEAMID)
```

The release workflow blocks public desktop releases until these secrets are configured. After they are configured, pushing a `v*` tag will produce:

```text
kaspa-miner-macos-universal.pkg
```

That package is signed, submitted with `xcrun notarytool`, stapled with `xcrun stapler`, and validated before upload.

The package installs:

```text
/usr/local/bin/kaspa-miner
/usr/local/share/kaspilot/start-mining.toml
/usr/local/share/kaspilot/config.example.toml
/usr/local/share/kaspilot/gpu.example.toml
/usr/local/share/kaspilot/fleet.example.toml
/usr/local/share/kaspilot/README.md
```

Users can create a local mining config with:

```sh
cp /usr/local/share/kaspilot/start-mining.toml ./config.toml
```
