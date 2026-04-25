# macOS No-Warning Release Setup

macOS does not provide a reliable no-warning path for unsigned downloaded binaries. For broad public distribution, ship the notarized installer package produced by the release workflow.

Required Apple account items:

- Active Apple Developer Program membership.
- Developer ID Application certificate.
- Developer ID Installer certificate.
- App-specific password for the Apple ID used by notarization.
- Apple Team ID.

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
