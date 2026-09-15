# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.3.x   | Yes       |
| < 0.3   | No        |

## Reporting a Vulnerability

If you discover a security vulnerability in modde, please report it responsibly:

1. **Do not** open a public issue
2. Email the maintainer directly or use Codeberg's private reporting feature
3. Include a description of the vulnerability, steps to reproduce, and potential impact

We will acknowledge receipt within 48 hours and aim to release a fix within 7 days for critical issues.

## Scope

modde handles:

- Nexus Mods API keys (stored via modde config, system keyring, or sops-nix)
- Local filesystem operations (symlinks, file copies)
- Network requests to mod hosting services

Security-relevant areas include API key storage, archive extraction (zip-slip prevention), and symlink handling (path traversal prevention).

## Verifying Shared Modlists

Portable `modde.lock` files are signed with Ed25519 detached signatures over the
canonical JSON payload. Verify a shared lock before importing it:

```sh
modde lock verify modde.lock --profile my-skyrim --game skyrim-se
```

Generate a signing key with:

```sh
modde lock keygen --public modde-lock.pub.json --secret modde-lock.secret.json
```

Keep `modde-lock.secret.json` private. Publish the public key through an
independent channel, and treat a changed public key as a key-rotation event.
`modde lock import` refuses unsigned or tampered locks; it does not invent
missing archives or source hashes.

## Verifying Releases

Every release is built from a GPG-signed Git tag. Release CI verifies the tag
against `keys/maintainers.gpg` before publishing artifacts.

Download the artifact you want, `SHA256SUMS.txt`, and
`SHA256SUMS.txt.minisig` from the same Codeberg release. Verify the signed
checksum manifest first:

```sh
minisign -Vm SHA256SUMS.txt -p keys/minisign.pub
sha256sum -c SHA256SUMS.txt --ignore-missing
```

The minisign public key is pinned in `keys/minisign.pub`. If this key changes,
treat the release as a key-rotation event and verify the new key from an
independent maintainer-controlled channel before trusting it.

Tarballs, AppImages, and source RPMs also ship with Sigstore signatures and
SLSA provenance:

```sh
cosign verify-blob \
  --bundle modde-<version>-x86_64-linux.tar.gz.cosign.bundle \
  --certificate-identity-regexp '.*caniko/rs-modde.*' \
  --certificate-oidc-issuer-regexp '.*' \
  modde-<version>-x86_64-linux.tar.gz

cosign verify-blob-attestation \
  --bundle modde-<version>-x86_64-linux.tar.gz.intoto.bundle \
  --type slsaprovenance1 \
  --certificate-identity-regexp '.*caniko/rs-modde.*' \
  --certificate-oidc-issuer-regexp '.*' \
  modde-<version>-x86_64-linux.tar.gz
```

The attestation payload is also published as
`modde-<version>-x86_64-linux.tar.gz.intoto.jsonl` for review tooling. Its SLSA
predicate records the source Git commit, `flake.lock` digest, release workflow
digest, and the `https://attic.candee.baby/canix` substituter trust root used
for release builds. The `builder.id` in the predicate is the release workflow
URI (`https://git.tartanoglu.com/caniko/rs-modde/.forgejo/workflows/release.yml@refs/tags/<version>`).

If you prefer the dedicated SLSA tooling over `cosign`, verify the same
provenance with `slsa-verifier`:

```sh
slsa-verifier verify-artifact \
  --provenance-path modde-<version>-x86_64-linux.tar.gz.intoto.jsonl \
  --source-uri github.com/caniko/rs-modde \
  --source-tag <version> \
  modde-<version>-x86_64-linux.tar.gz
```

Both `nix shell nixpkgs#cosign` and `nix shell nixpkgs#slsa-verifier` provide
the verifier binaries without a separate install.

If Forgejo Actions OIDC is accepted by Sigstore, CI uses keyless Fulcio/Rekor
signing. If that is unavailable, CI falls back to the `COSIGN_PRIVATE_KEY` and
`COSIGN_PASSWORD` Forgejo secrets; the release audit report records this
deviation. Minisign key rotation requires generating a new offline keypair,
updating `keys/minisign.pub`, replacing the Forgejo `MINISIGN_SECRET_KEY` and
`MINISIGN_PASSWORD` secrets, and publishing a signed release note that names
both the old and new public keys.

### The signed Git tag

Every release is cut from a GPG-signed annotated tag. The maintainer public
keys live in `keys/maintainers.gpg`, and the release workflow enforces
`git verify-tag <version>` against that keyring before any artifact is built —
an unsigned or unrecognized tag fails the release at the validation step. To
check a tag yourself from a clean checkout:

```sh
gpg --import keys/maintainers.gpg
git verify-tag <version>
```

### macOS Gatekeeper

modde does **not** notarize its macOS builds and does not have an Apple
Developer ID signing certificate; macOS artifacts are experimental and there is
no published Codeberg release asset for them yet. The integrity check for a
macOS download is the same minisign/cosign verification described above against
the tarball — not an Apple notarization ticket.

Because the binaries are neither signed by an Apple Developer ID nor notarized,
Gatekeeper quarantines them on first launch. After verifying the download,
clear the quarantine attribute once:

```sh
xattr -d com.apple.quarantine modde
xattr -d com.apple.quarantine modde-ui
```

Alternatively, right-click the binary in Finder and choose **Open** the first
time to approve it through the Gatekeeper dialog. Both approaches are one-time
per download; subsequent launches run normally. Verify the minisign signature
first so the bytes you are de-quarantining are the ones the maintainer signed.

## APT Repository Signing Key

The Debian/Ubuntu APT repository (served at `https://modde.rs/apt/` once the
channel is live; not yet published) is signed with a dedicated repository GPG
key, separate from the maintainer tag-signing key and the minisign
release-manifest key. The repository signing key fingerprint is:

```
CCFE4A8461DF8778F5227684B6DB8F177A951E1B
```

That fingerprint is the trust anchor for the APT channel; pin it rather than
trusting the served key file blindly. The public key is published at
`dist/apt/key.gpg.asc` (and, once the channel is live, at
`https://modde.rs/apt/key.gpg.asc`). Install the repository key into a keyring
and confirm the fingerprint before adding the source entry:

```sh
install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://modde.rs/apt/key.gpg.asc | gpg --dearmor > /etc/apt/keyrings/modde.gpg
# Confirm the fingerprint matches before trusting the key:
gpg --show-keys --with-colons /etc/apt/keyrings/modde.gpg \
  | awk -F: '/^fpr:/ {print $10; exit}'
# Expect: CCFE4A8461DF8778F5227684B6DB8F177A951E1B
echo "deb [signed-by=/etc/apt/keyrings/modde.gpg] https://modde.rs/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/modde.list
```

The APT `Release` file must be signed with this same key; a fingerprint
mismatch surfaces as a `NO_PUBKEY` or `signed-by` error on `apt update`.

APT repository signing-key rotation is independent from release transport. To
rotate the signing key, generate a new repository-only GPG key, update the
public key published at `dist/apt/key.gpg.asc`, replace the
`modde_apt_repo_gpg_key`, `modde_apt_repo_gpg_key_id`, and optional
`modde_apt_repo_gpg_passphrase` runner secrets together, publish a signed
release note that names both old and new fingerprints, and keep the old public
key available long enough for users to migrate. Rotate
`modde_apt_repo_ssh_key` only if the APT repository push key itself is
compromised.

### APT Repository Push Key

APT repository publication uses a per-repository ed25519 SSH deploy key on
`caniko/apt-modde`, not a Codeberg access token. The private key is stored as
the canix-managed `modde_apt_repo_ssh_key` runner credential and is exposed to
release CI as `APT_REPO_SSH_KEY`; the matching public key is registered on
`caniko/apt-modde` with write access. To rotate the push credential, generate
a fresh ed25519 key pair, store the private half as the `modde_apt_repo_ssh_key`
canix runner credential (surfaced to CI as `APT_REPO_SSH_KEY`), and register the
new public half as a write-access deploy key on `caniko/apt-modde`.

The repository bootstrap is complete, but the first APT publication must not
run until the matching public key is confirmed on that repository. Validate the
attachment with the Codeberg deploy-key view and an SSH `git ls-remote`/push
preflight using the managed credential; do not replace this per-repository key
with the general Codeberg account token.

## Windows Code Signing

Windows release artifacts are Authenticode-signed after the Nix
`modde-windows` build and before tar/zip packaging, checksum generation, and
Sigstore signing. The Nix output itself remains unsigned because private
certificate material must not enter the Nix store; the user-facing Windows
artifacts are the files in the Codeberg release.

The selected CI path for the current Forgejo `atlas` runner is a publicly
trusted Authenticode certificate exported as a PKCS#12 bundle and supplied only
through Forgejo secrets:

- `WINDOWS_SIGNING_PFX`: base64-encoded `.p12` / `.pfx` certificate bundle
- `WINDOWS_SIGNING_PASS`: passphrase for the certificate bundle
- `WINDOWS_SIGNING_SUBJECT`: exact expected signer subject substring, such as
  the certificate's `CN=...`

Azure Artifact Signing, formerly Azure Trusted Signing, remains the preferred
future direction once a Windows signing runner exists. Microsoft's supported
Artifact Signing integration uses Windows SignTool with the Artifact Signing
dlib, and the official action is Windows-runner-only. The Linux `atlas` runner
therefore uses `osslsigncode` instead of pretending the Azure flow can run
there today.

When the Windows signing secrets are absent, CI emits a warning and publishes
unsigned Windows artifacts so pull requests and non-signing dry runs keep
passing. A production release should treat that warning as a release blocker.

Verify a signed release on Linux:

```sh
tar xzf modde-<version>-x86_64-windows.tar.gz
osslsigncode verify -in modde.exe
osslsigncode verify -in modde-ui.exe
```

Verify on Windows PowerShell:

```powershell
Get-AuthenticodeSignature .\modde.exe
Get-AuthenticodeSignature .\modde-ui.exe
```

Both commands should report `Status : Valid`. The signer subject must match the
`WINDOWS_SIGNING_SUBJECT` value configured for release CI and recorded in the
release notes for that certificate generation.

Certificate rotation procedure:

1. Order or renew the public CA Authenticode certificate before the old
   certificate expires. For an OV certificate, start at least 60 days before
   expiry so Windows SmartScreen reputation can overlap.
2. Export the new certificate chain and private key as a password-protected
   PKCS#12 file, then encode it without line wrapping:
   `base64 -w0 modde-code-signing.p12`.
3. Replace the Forgejo `WINDOWS_SIGNING_PFX`, `WINDOWS_SIGNING_PASS`, and
   `WINDOWS_SIGNING_SUBJECT` secrets together. Do not commit the certificate,
   private key, passphrase, or decoded bundle.
4. Run a dry release or throwaway tag and confirm `osslsigncode verify` passes
   on both staged `.exe` files and on the extracted tarball contents.
5. On a fresh Windows 11 machine, confirm `Get-AuthenticodeSignature` reports
   `Valid` and that the displayed signer matches `WINDOWS_SIGNING_SUBJECT`.
6. Keep the old certificate valid until the new certificate has shipped and,
   for OV certificates, has accumulated SmartScreen reputation. Timestamped
   releases use `http://timestamp.digicert.com` so already-shipped signatures
   remain valid after normal certificate expiry.

Signed Windows binaries are intentionally not bit-reproducible because
Authenticode injects a timestamped signature after the Nix build. Linux and
unsigned Nix artifacts can still be rebuilt from source; Windows users should
verify both the signed checksum manifest and the Authenticode signature.

MSIX packaging is deferred to a separate distribution enhancement.

## Inspecting the SBOM

Each Codeberg release includes machine-readable software bills of materials:

- `modde-<version>.cdx.json`: CycloneDX JSON generated from the workspace `Cargo.lock`
- `modde-<version>.spdx.json`: SPDX 2.3 JSON generated from the workspace `Cargo.lock`

Scan the CycloneDX SBOM with Grype:

```sh
grype "sbom:modde-<version>.cdx.json"
```

Or scan either SBOM with OSV-Scanner:

```sh
osv-scanner scan --sbom=modde-<version>.cdx.json
osv-scanner scan --sbom=modde-<version>.spdx.json
```

Release CI also runs `cargo deny check -W unmaintained advisories bans sources licenses`; vulnerable Rust dependencies fail the release unless an explicit `deny.toml` advisory ignore documents the temporary exception.

## Announcement Credentials

Release announcements use dedicated Forgejo secrets. Create narrow tokens for
the announcement account only; never reuse a maintainer's personal token.

- Mastodon: create an application token for the `@modde@fosstodon.org` account
  with `write:statuses`, then set `MASTODON_TOKEN` and `MASTODON_BASE_URL`.
- Matrix: create or rotate an access token for the release bot account, invite
  that account to the release room, then set `MATRIX_TOKEN`,
  `MATRIX_HOMESERVER`, and `MATRIX_ROOM`.

If a token is suspected to be exposed, revoke it at the upstream service, rotate
the Forgejo secret, and re-run only the announcement step manually if the
release itself already published correctly.
