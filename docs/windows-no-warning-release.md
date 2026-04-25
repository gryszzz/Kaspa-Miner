# Windows No-Warning Release Setup

Windows SmartScreen reputation is tied to trusted Authenticode signing and download reputation. The release workflow signs Windows binaries before packaging them.

Required certificate item:

- Code-signing certificate exported as `.pfx`.

Recommended for the cleanest public launch:

- EV code-signing certificate when available.
- Long-lived timestamping on every signature.
- Consistent publisher name across releases.

Required GitHub Actions secrets:

```text
WINDOWS_CERTIFICATE_PFX_BASE64
WINDOWS_CERTIFICATE_PASSWORD
```

The `WINDOWS_CERTIFICATE_PFX_BASE64` secret should contain a base64-encoded `.pfx` export that includes the private key.

Example local encoding command from PowerShell:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("kaspilot-signing.pfx")) | Set-Clipboard
```

The release workflow uses Microsoft `signtool.exe` with SHA-256 digesting and DigiCert timestamping, then verifies the signature before packaging.
