# APT Repository Plan

Status: decisions made, ready for phase 1 (2026-11-04). Publishes the LaanKit `.deb` packages (ThreadLiner, PageLiner, NoteLiner, and BlockLiner) as a signed APT repository at `https://apt.laan.dev`, hosted on GitHub Pages, so you can install them with `apt install` and update them with `apt upgrade`.

## Review of the generic plan

The generic plan (GitHub Actions, then reprepro, then a signed repository on GitHub Pages) is sound and fits LaanKit. Applied to this repo, most of its step 1 and step 3 already exist, and a few points need changing.

| Generic plan | LaanKit today | Change |
|---|---|---|
| Step 1: add Debian packaging metadata | Done: `[package.metadata.deb]` in each `apps/<app>/native/Cargo.toml`, built by `cargo-deb` through `scripts/package-native.sh` | None for amd64. Add lintian and an install test. |
| Step 3: build on version tags, upload artifacts, attach to Releases | Done: `release-native.yml` runs on `native-v*` tags and attaches every package to a draft release | Switch to per-app tags (`<app>-v<version>`) that build one app, and add an install and uninstall test before the draft release. |
| Step 4: reprepro or aptly | Not started | Use `apt-ftparchive` instead (see below). |
| Step 5: publish `pool/` and `dists/` to Pages | Not started | Use a separate `NathanLaan/apt` repository, deployed with `actions/deploy-pages`, so no `.deb` files are committed to git. |
| Supported distributions | CI builds on Ubuntu 24.04, so `$auto` makes each package depend on glibc 2.39 or newer | Support Debian 13 and newer (glibc 2.41); keep building on Ubuntu 24.04. |
| Architectures (amd64 and arm64) | amd64 only | Stay amd64 only. arm64 is a later phase. |

### Why apt-ftparchive rather than reprepro

reprepro keeps a database and, by default, one version of each package per suite. That suits a server with persistent state, but a GitHub Pages site built from a workflow has no state between runs unless you commit the database and the pool to git. `apt-ftparchive` (from `apt-utils`) is stateless: it scans a `pool/` folder and writes `Packages` and `Release` for whatever is there, including several versions of one package. That lets the workflow rebuild the whole repository from GitHub Releases on every run, which keeps the published repository and the releases in step and keeps binaries out of git history. aptly would also work, but it's a larger tool for no gain here.

### Why a separate repository

The repository lives in a new public GitHub repository, `NathanLaan/apt`, not in `laankit` itself.

- A GitHub repository has one Pages site. A separate one keeps `laankit`'s Pages free for a project site and lets `apt.laan.dev` carry packages from other projects later.
- The signing key is a secret of `NathanLaan/apt` only, so the `laankit` build workflows never see it.
- The `.deb` files never enter `laankit`'s git history.

## Architecture

```
laankit: <app>-v<version> tag (for example blockliner-v2.6.5)
  -> release-native.yml: build that app, lintian, install test, draft release
  -> you publish the release by hand (the existing manual gate)
  -> notify-apt.yml (on release: published): repository_dispatch to NathanLaan/apt

NathanLaan/apt: publish.yml (on dispatch, by hand, or weekly)
  -> download the .deb files of the latest published releases
  -> pool/, prune to the newest 3 versions per package
  -> apt-ftparchive: Packages, Packages.gz, Packages.xz, Release
  -> gpg: InRelease, Release.gpg
  -> install test from the built site in Debian and Ubuntu containers
  -> actions/deploy-pages -> https://apt.laan.dev
```

### Published layout

| Path | What |
|---|---|
| `/dists/stable/InRelease` | Signed `Release` (inline signature) |
| `/dists/stable/Release`, `/dists/stable/Release.gpg` | `Release` with a detached signature, for older clients |
| `/dists/stable/main/binary-amd64/Packages{,.gz,.xz}` | Package indexes |
| `/pool/main/<letter>/<package>/<package>_<version>_amd64.deb` | The packages, for example `pool/main/n/noteliner/noteliner_2.3.1-1_amd64.deb` |
| `/laan.dev.gpg` | The public key, binary (dearmored), ready for `/etc/apt/keyrings/` |
| `/laan.dev.asc` | The same key, ASCII-armoured |
| `/laan.dev.sources` | A ready-made deb822 sources file |
| `/index.html` | Install instructions and the key fingerprint |

One suite, `stable`, with one component, `main`, serves every supported release: the binaries depend only on glibc and on libraries Debian 13 and newer ship, so there's no need for per-codename suites (`trixie`, `forky`, and so on). The key and file names use `laan.dev` rather than `laankit` so the same repository can carry other projects.

## Decisions

| # | Decision | Choice |
|---|---|---|
| 1 | Version scheme | Per-app tags and versions. Each app has its own version in `apps/<app>/native/Cargo.toml` and is released with its own tag, `<app>-v<version>` (for example `blockliner-v2.6.5`), which builds and releases only that app. This also guarantees the repository never sees two different files with the same package name and version. |
| 2 | Supported distributions | Debian 13 (trixie) and newer. CI keeps building on Ubuntu 24.04 (glibc 2.39), which Debian 13's glibc 2.41 satisfies. Other Debian-based distributions with glibc 2.39 or newer (such as Ubuntu 24.04) will likely work but aren't tested or supported. |
| 3 | Architectures | amd64 only for now. arm64 is phase 6. |
| 4 | Signing key | An Ed25519 signing subkey, which apt's `sqv` and `gpgv` accept on Debian 13. You keep the primary key and the master copy of the subkey in your own separate keystore; CI holds only an export of the signing subkey. Subkey expiry of 2 years, extended before it lapses. |
| 5 | Pre-releases (`2.4.0-rc.1`) | Left out of APT for now (the build skips releases marked as pre-releases). If added later, use a `testing` suite and check that `cargo-deb` writes them as `2.4.0~rc.1` so they sort before `2.4.0`. |
| 6 | Retention | The newest 3 versions of each package, so you can downgrade with `apt install noteliner=<version>`. With per-app releases each app keeps its own 3, independent of how often the others are released. |

## Phase 1: Signing key, GitHub, and domain

Done by hand on your machine, in GitHub, and at your DNS provider. Nothing here runs in CI except the check in step 1.10. When phase 1 is done:

- the signing key exists in your keystore, with offline backups of the primary key and the revocation certificate;
- a CI copy of the signing subkey, with its own passphrase, is stored as environment secrets in `NathanLaan/apt`;
- `https://apt.laan.dev` serves a placeholder page over HTTPS from GitHub Pages;
- `laankit` holds a token that can trigger the `NathanLaan/apt` workflows.

### Before you start

| Need | Check |
|---|---|
| GnuPG 2.2 or newer | `gpg --version` |
| GitHub CLI, logged in as `NathanLaan` with the `repo` and `workflow` scopes | `gh auth status` |
| `dig` (Debian package `bind9-dnsutils`) | `dig -v` |
| `openssl` | `openssl version` |
| Access to the DNS settings for `laan.dev` | |
| An offline backup location (for example an encrypted USB drive) | |
| A password manager for the CI passphrase and the dispatch token | |

Run steps 1.1 to 1.6 in one terminal session: they share shell variables. If you open a new terminal partway through, run this block first to set them again (it works once step 1.1 has created the key):

```bash
export GNUPGHOME=~/.gnupg
UID_STR='laan.dev APT repository <apt@laan.dev>'
WORK=~/laan.dev-apt
FPR=$(gpg --list-keys --with-colons "$UID_STR" | awk -F: '/^fpr:/ {print $10; exit}')
SUBKEY_FPR=$(gpg --list-keys --with-colons "$FPR" | awk -F: '/^sub:/ {s=1; next} s && /^fpr:/ {print $10; exit}')
echo "primary: $FPR"; echo "signing subkey: $SUBKEY_FPR"
```

### 1.1 Create the key

In your keystore, create an Ed25519 certify-only primary key and an Ed25519 signing subkey that expires in 2 years. The primary key never leaves the keystore. Set `GNUPGHOME` to the keystore's folder, and replace the email address with the one you want on the key (it's shown to anyone who inspects the key, so use an address you'll keep).

```bash
export GNUPGHOME=~/.gnupg
install -d -m 0700 "$GNUPGHOME"
UID_STR='laan.dev APT repository <apt@laan.dev>'
WORK=~/laan.dev-apt            # public files only; nothing secret goes here
mkdir -p "$WORK"

# Primary key: certify only, no expiry. gpg asks for the keystore passphrase
# and writes a revocation certificate to $GNUPGHOME/openpgp-revocs.d/<fingerprint>.rev.
gpg --quick-generate-key "$UID_STR" ed25519 cert never
FPR=$(gpg --list-keys --with-colons "$UID_STR" | awk -F: '/^fpr:/ {print $10; exit}')

# Signing subkey: Ed25519, expires in 2 years. gpg asks for the keystore passphrase.
gpg --quick-add-key "$FPR" ed25519 sign 2y
SUBKEY_FPR=$(gpg --list-keys --with-colons "$FPR" | awk -F: '/^sub:/ {s=1; next} s && /^fpr:/ {print $10; exit}')

# Check: "pub ed25519 [C]" with no expiry, then "sub ed25519 [S] [expires: <2 years>]".
gpg --list-keys --with-subkey-fingerprints "$FPR"
echo "primary: $FPR"; echo "signing subkey: $SUBKEY_FPR"
```

The subkey shares the primary key's passphrase. This is your keystore passphrase; it never goes to CI (step 1.4 gives the CI copy its own).

### 1.2 Back up the primary key and the revocation certificate

Without the primary key you can't extend the subkey's expiry or create a new subkey, so every client would need a new key. Without the revocation certificate you can't revoke the key if the keystore is lost or stolen. Keep both somewhere other than the keystore.

```bash
BACKUP=/path/to/offline/backup     # for example the mounted encrypted USB drive

# The full secret key (primary and subkey), still protected by the keystore passphrase.
gpg --armor --export-secret-keys "$FPR" > "$BACKUP/laan.dev-apt-secret-keys.asc"
cp "$GNUPGHOME/openpgp-revocs.d/$FPR.rev" "$BACKUP/laan.dev-apt-revocation.rev"

# Check: lists "sec" (not "sec#") with both keys.
gpg --show-keys "$BACKUP/laan.dev-apt-secret-keys.asc"
```

Optionally, also print a paper copy: `gpg --export-secret-keys "$FPR" | paperkey --output laan.dev-apt-paperkey.txt` (package `paperkey`), print it, and delete the file.

### 1.3 Export the public key

```bash
# Binary key for /etc/apt/keyrings/ and ASCII-armoured key for people to read.
gpg --export-options export-minimal --export "$FPR" > "$WORK/laan.dev.gpg"
gpg --export-options export-minimal --armor --export "$FPR" > "$WORK/laan.dev.asc"

# Check: the primary and the [S] subkey, with the fingerprints from step 1.1.
gpg --show-keys --with-subkey-fingerprints "$WORK/laan.dev.gpg"

# The fingerprint in the grouped form used in the docs.
gpg --fingerprint "$FPR" | sed -n 2p
```

Both files are public. They go into `static/` in `NathanLaan/apt` (`laan.dev.gpg` in step 1.10, both in phase 3).

### 1.4 Create the CI copy of the signing subkey

CI gets only the signing subkey, protected by a passphrase of its own, so the CI secrets never reveal your keystore passphrase. The export carries a stub in place of the primary secret key, so CI can sign but can't certify new subkeys or change expiry.

1. Generate the CI passphrase and save it in your password manager as "laan.dev APT CI passphrase":

   ```bash
   openssl rand -base64 32
   ```

2. Export the subkey, re-protect it with the CI passphrase in a temporary keyring in RAM (`/dev/shm`), and export it again:

   ```bash
   # Export the signing subkey only (the ! limits it to that subkey).
   # gpg asks for the keystore passphrase.
   gpg --export-secret-subkeys "$SUBKEY_FPR!" > /dev/shm/apt-subkey.gpg

   TMPHOME=$(mktemp -d -p /dev/shm)
   gpg --homedir "$TMPHOME" --import /dev/shm/apt-subkey.gpg

   # gpg asks for the keystore passphrase, then the new passphrase twice: paste the
   # CI passphrase. It reports "error changing passphrase: No secret key" for the
   # primary key, which is expected: only the subkey is in this keyring.
   gpg --homedir "$TMPHOME" --edit-key "$FPR" passwd save

   # Check: "sec#" (primary absent) and "ssb ed25519 [S]".
   gpg --homedir "$TMPHOME" --list-secret-keys

   # Check the new passphrase: restart the agent so nothing is cached, sign a test
   # message (gpg asks for the CI passphrase), and verify it against the public key.
   gpgconf --homedir "$TMPHOME" --kill gpg-agent
   echo test | gpg --homedir "$TMPHOME" --clearsign | gpgv --keyring "$WORK/laan.dev.gpg"

   # The CI copy. gpg asks for the CI passphrase.
   gpg --homedir "$TMPHOME" --armor --export-secret-subkeys "$SUBKEY_FPR!" > /dev/shm/apt-signing-subkey.asc
   ```

Keep the terminal open: step 1.6 stores `/dev/shm/apt-signing-subkey.asc` and then deletes it.

### 1.5 Create the repository

```bash
gh repo create NathanLaan/apt --public --add-readme \
  --description "APT repository for laan.dev packages (https://apt.laan.dev)"
gh repo clone NathanLaan/apt ~/dev/hub/laan/apt
```

The clone path is an example; later steps call it `~/dev/hub/laan/apt`.

### 1.6 Create the signing environment and its secrets

The signing secrets are environment secrets, not repository secrets: only a job that declares `environment: apt-signing` can read them, and only on `main`, so a workflow on another branch or a pull request can't sign with the key.

```bash
# The environment, limited to branches you name.
gh api -X PUT repos/NathanLaan/apt/environments/apt-signing --input - <<'EOF'
{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}
EOF
# Allow main only.
gh api -X POST repos/NathanLaan/apt/environments/apt-signing/deployment-branch-policies \
  -f name=main -f type=branch

# The secrets. The second command asks for the value: paste the CI passphrase.
gh secret set APT_SIGNING_KEY --repo NathanLaan/apt --env apt-signing < /dev/shm/apt-signing-subkey.asc
gh secret set APT_SIGNING_PASSPHRASE --repo NathanLaan/apt --env apt-signing

# Check: both secrets listed, and "main" as the only branch.
gh secret list --repo NathanLaan/apt --env apt-signing
gh api repos/NathanLaan/apt/environments/apt-signing/deployment-branch-policies --jq '.branch_policies[].name'

# Delete the temporary keyring and both exports.
gpgconf --homedir "$TMPHOME" --kill gpg-agent
rm -rf "$TMPHOME" /dev/shm/apt-subkey.gpg /dev/shm/apt-signing-subkey.asc
```

The same settings are in the web UI under the repository's Settings, Environments, `apt-signing`: "Deployment branches and tags" set to "Selected branches and tags" with `main`, and the two secrets under "Environment secrets".

### 1.7 Verify the laan.dev domain with GitHub

Verifying the domain stops any other GitHub account from claiming `apt.laan.dev` (or any other subdomain) for its own Pages site, for example if the repository's Pages site is ever turned off while the DNS record still points to GitHub.

1. Open `https://github.com/settings/pages`, choose "Add a domain", and enter `laan.dev`.
2. GitHub shows a `TXT` record. At your DNS provider, add it as shown:

   | Type | Name | Value |
   |---|---|---|
   | `TXT` | `_github-pages-challenge-NathanLaan` | The code GitHub shows |

3. Wait until the record resolves:

   ```bash
   dig +short TXT _github-pages-challenge-nathanlaan.laan.dev
   ```

4. Back on the GitHub page, choose "Verify".

Leave the `TXT` record in place: GitHub checks it again from time to time, and removing it un-verifies the domain.

### 1.8 Turn on Pages with the custom domain

Do this before adding the DNS record in step 1.9, so the subdomain is claimed by your repository before it points to GitHub.

```bash
# Pages, deployed by GitHub Actions (this also creates the github-pages environment).
gh api -X POST repos/NathanLaan/apt/pages -f build_type=workflow
# The custom domain.
gh api -X PUT repos/NathanLaan/apt/pages -f cname=apt.laan.dev

# Check: build_type "workflow" and cname "apt.laan.dev".
gh api repos/NathanLaan/apt/pages --jq '{build_type, cname, https_enforced}'
```

In the web UI this is the repository's Settings, Pages: "Source" set to "GitHub Actions" and "Custom domain" set to `apt.laan.dev`.

### 1.9 Add the DNS record

At your DNS provider:

| Type | Name | Value | TTL |
|---|---|---|---|
| `CNAME` | `apt` | `nathanlaan.github.io.` | 3600 (or the provider's default) |

If the provider can proxy traffic (Cloudflare's orange cloud, for example), set the record to DNS only: GitHub has to see its own servers to issue the HTTPS certificate.

```bash
# Check: nathanlaan.github.io. followed by GitHub's 185.199.108-111.153 addresses.
dig +short apt.laan.dev
```

GitHub then requests a Let's Encrypt certificate for `apt.laan.dev`. This usually takes minutes but can take up to 24 hours. The repository's Settings, Pages shows "DNS check successful" once it sees the record.

### 1.10 Deploy a placeholder and check the signing secrets

A first deployment proves the domain, HTTPS, and the Pages workflow, and a signing job proves the environment secrets, before any packages are involved. Phase 3 replaces this workflow.

In the clone from step 1.5, add the public key and the workflow:

```bash
cd ~/dev/hub/laan/apt
mkdir -p static .github/workflows
cp "$WORK/laan.dev.gpg" static/
```

`.github/workflows/publish.yml`:

```yaml
# Placeholder: checks the signing secrets and deploys a holding page.
# Replaced by the real build in phase 3.
name: publish
on: workflow_dispatch

permissions:
  contents: read

jobs:
  check-signing:
    runs-on: ubuntu-24.04
    environment: apt-signing
    steps:
      - uses: actions/checkout@v5
      - name: Sign a test message and verify it with the public key
        env:
          APT_SIGNING_KEY: ${{ secrets.APT_SIGNING_KEY }}
          APT_SIGNING_PASSPHRASE: ${{ secrets.APT_SIGNING_PASSPHRASE }}
        run: |
          export GNUPGHOME=$(mktemp -d)
          echo "$APT_SIGNING_KEY" | gpg --batch --import
          echo test | gpg --batch --pinentry-mode loopback --passphrase-fd 3 --clearsign 3<<<"$APT_SIGNING_PASSPHRASE" > test.asc
          gpgv --keyring "$PWD/static/laan.dev.gpg" test.asc

  deploy:
    needs: check-signing
    runs-on: ubuntu-24.04
    permissions:
      pages: write
      id-token: write
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    concurrency:
      group: pages
      cancel-in-progress: false
    steps:
      - name: Holding page
        run: |
          mkdir site
          echo '<!doctype html><title>apt.laan.dev</title><p>The laan.dev APT repository is coming soon.</p>' > site/index.html
      - uses: actions/upload-pages-artifact@v4
        with:
          path: site
      - uses: actions/deploy-pages@v4
        id: deployment
```

Commit, push, and run it:

```bash
git add static/laan.dev.gpg .github/workflows/publish.yml
git commit -m "Placeholder Pages deployment and signing check"
git push
gh workflow run publish.yml --repo NathanLaan/apt
gh run watch --repo NathanLaan/apt      # choose the run; both jobs should pass

# Check: "HTTP/2 200".
curl -sI https://apt.laan.dev/ | head -1
```

If `check-signing` fails at the import, the `APT_SIGNING_KEY` secret is incomplete (it must include the `-----BEGIN` and `-----END` lines). If it fails at signing, `APT_SIGNING_PASSPHRASE` doesn't match the passphrase from step 1.4. If `curl` fails with a certificate error, the certificate isn't issued yet: wait and try again.

Once `curl` works, enforce HTTPS:

```bash
gh api -X PUT repos/NathanLaan/apt/pages -F https_enforced=true
gh api repos/NathanLaan/apt/pages --jq .https_enforced     # true
```

`.dev` domains are on the browsers' HSTS preload list, so browsers use HTTPS regardless; enforcing it also redirects any plain HTTP client, such as an apt source mistyped as `http://`.

### 1.11 Create the dispatch token for laankit

`laankit` needs a token to start the `NathanLaan/apt` workflow when you publish a release (phase 2). The default `GITHUB_TOKEN` can't reach another repository.

1. Open `https://github.com/settings/personal-access-tokens/new` and fill in:

   | Field | Value |
   |---|---|
   | Token name | `laankit to apt dispatch` |
   | Resource owner | `NathanLaan` |
   | Expiration | 1 year (the longest you're comfortable with) |
   | Repository access | Only select repositories: `NathanLaan/apt` |
   | Repository permissions | Contents: Read and write (Metadata: Read-only is added automatically) |

2. Choose "Generate token" and copy it. Save it in your password manager.
3. Store it in `laankit` (a repository secret is enough: the token can only start workflows in `NathanLaan/apt`, and the signing key stays behind the `apt-signing` environment):

   ```bash
   gh secret set APT_DISPATCH_TOKEN --repo NathanLaan/laankit     # paste the token
   ```

4. Check that it can send a dispatch. Nothing listens for `ping`, so this does nothing except prove the token works (no output and exit status 0 means GitHub accepted it):

   ```bash
   read -rs TOKEN                                   # paste the token, then Enter
   GH_TOKEN=$TOKEN gh api repos/NathanLaan/apt/dispatches -f event_type=ping && echo accepted
   unset TOKEN
   ```

A GitHub App installed only on `NathanLaan/apt` is the alternative if you'd rather not renew a personal token each year.

### 1.12 Record the details

Keep these in your password manager or notes. The fingerprint is also published in phase 4.

| Item | Where to find it |
|---|---|
| Primary key fingerprint | `gpg --fingerprint "$FPR" \| sed -n 2p` |
| Signing subkey fingerprint and expiry | `gpg --list-keys --with-subkey-fingerprints "$FPR"` |
| Subkey renewal reminder | 3 months before the subkey expires (see phase 6) |
| Dispatch token expiry and renewal reminder | The date shown when the token was created; renew a week before |
| Backup location | Step 1.2 |
| CI passphrase | Password manager, from step 1.4 |

### Phase 1 checklist

| Check | Expected |
|---|---|
| `gpg --list-keys --with-subkey-fingerprints "$FPR"` | `pub ed25519 [C]`, no expiry; `sub ed25519 [S]`, expires in 2 years |
| Offline backup | Secret keys and revocation certificate present; `gpg --show-keys` reads the secret keys file |
| `ls /dev/shm/apt-*` | Nothing: the CI exports are deleted |
| `gh secret list --repo NathanLaan/apt --env apt-signing` | `APT_SIGNING_KEY`, `APT_SIGNING_PASSPHRASE` |
| `apt-signing` deployment branches | `main` only |
| `https://github.com/settings/pages` | `laan.dev` verified |
| `gh api repos/NathanLaan/apt/pages --jq '{build_type, cname, https_enforced}'` | `workflow`, `apt.laan.dev`, `true` |
| `curl -sI https://apt.laan.dev/` | `HTTP/2 200` |
| Last `publish` run | `check-signing` and `deploy` passed |
| `gh secret list --repo NathanLaan/laankit` | `APT_DISPATCH_TOKEN` |

## Phase 2: Per-app releases and package checks in laankit

Changes to `laankit` so each app is released on its own, and every published `.deb` has been tested before it reaches a release.

### Per-app tags and versions

| File | Change |
|---|---|
| `scripts/set-version.sh` | Takes the app as well: `scripts/set-version.sh blockliner 2.6.6`. Updates only that app's `Cargo.toml` and `packaging/linux/<app id>.metainfo.xml`, then its `Cargo.lock` entry. |
| `.github/workflows/release-native.yml` | Triggers on `threadliner-v*`, `pageliner-v*`, `noteliner-v*`, and `blockliner-v*` (dropping `native-v*`), plus `workflow_dispatch` with an `app` input. A first step splits the tag into app and version, fails if the version doesn't match `apps/<app>/native/Cargo.toml`, and passes the app to `scripts/package-native.sh <app>` (which already accepts one app). |
| `.github/workflows/release-native.yml` | The draft release is named after the app and version ("BlockLiner 2.6.5"), and only that app's packages are attached. |
| `crates/laankit-core/src/updates.rs` | The update check reads the newest release whose tag starts with the app's prefix (`noteliner-v`), not the newest release in the repository, and strips that prefix before comparing versions. NoteLiner's `UPDATE_REPO` changes from `NathanLaan/noteliner` to `NathanLaan/laankit`; ThreadLiner and PageLiner can turn the check on the same way. The query should fetch more than 10 releases (or page through them), since four apps now share one release list. |
| `docs/packaging.md` | The "Versions" section describes per-app versions and tags. |

### Package checks

| File | Change |
|---|---|
| `.github/workflows/release-native.yml` | After `package` and before `draft-release`, a `test-deb` job in a `debian:13` container: `apt-get install -y ./<pkg>.deb` (resolves its dependencies), `ldd /usr/bin/<app>` with no "not found", `dpkg -L` lists the desktop entry, metainfo, and icons, then `apt-get purge -y <pkg>` and check that nothing under `/usr` is left. Add `debian:testing` as a non-blocking job to catch problems with the next release early. |
| `.github/workflows/release-native.yml` | `lintian` on the `.deb` in the `package` job, failing on errors only at first (warnings such as `no-manual-page` are expected). |
| `.github/workflows/notify-apt.yml` (new) | On `release: published` (and by hand): `gh api repos/NathanLaan/apt/dispatches -f event_type=laankit-release` with `APT_DISPATCH_TOKEN`. This runs only when you publish a draft yourself, which keeps the existing manual gate. |
| `apps/*/native/Cargo.toml` | Optional: a `changelog` asset (`usr/share/doc/<pkg>/changelog.Debian.gz`) to quiet lintian, if it's cheap to generate from the metainfo releases. |

## Phase 3: The apt repository

The contents of `NathanLaan/apt`.

| Path | What |
|---|---|
| `sources.txt` | The GitHub repositories to collect packages from, one per line (`NathanLaan/laankit` to start) |
| `scripts/build-repo.sh` | Builds `site/` (below) |
| `conf/release.conf` | `apt-ftparchive` settings for `Release`: `Origin: laan.dev`, `Label: laan.dev`, `Suite: stable`, `Codename: stable`, `Architectures: amd64`, `Components: main`, `Description` |
| `static/` | `index.html`, `laan.dev.asc`, `laan.dev.gpg`, `laan.dev.sources`, and `CNAME` (`apt.laan.dev`) |
| `.github/workflows/publish.yml` | Runs on `repository_dispatch`, by hand, and weekly (so a missed dispatch is recovered) |
| `README.md` | How the repository is built, the key fingerprint, and the key rotation steps |

### build-repo.sh

1. For each repository in `sources.txt`, list published releases that aren't drafts or pre-releases (`gh release list --exclude-drafts --exclude-pre-releases --limit 100 --json tagName`) and download their `.deb` assets (`gh release download <tag> -p '*.deb'`). Each per-app release carries one `.deb`, so the limit has to cover at least 3 releases of every app; the pruning in step 3 does the rest. No token beyond the default `GITHUB_TOKEN` is needed: the releases are public.
2. Read each file's `Package`, `Version`, and `Architecture` with `dpkg-deb -f`, and file it under `pool/main/<letter>/<package>/`. If two releases carry a file with the same name but different contents, stop with an error rather than pick one: apt clients that cached the first checksum would fail.
3. Keep the newest 3 versions of each package (sorted with `dpkg --compare-versions`) and drop the rest.
4. `apt-ftparchive packages pool/ > dists/stable/main/binary-amd64/Packages`, then write `Packages.gz` and `Packages.xz`.
5. `apt-ftparchive -c conf/release.conf release dists/stable > dists/stable/Release`.
6. Sign with the subkey: `gpg --clearsign` to `InRelease` and `gpg --armor --detach-sign` to `Release.gpg`, then check both with `gpgv --keyring static/laan.dev.gpg`, so a wrong key fails the build rather than every client.
7. Copy `static/` into the site root.

### publish.yml

| Job | Steps |
|---|---|
| `build` | Runs in the `apt-signing` environment (for the signing secrets). Checkout, install `apt-utils`, `gnupg`, and `xz-utils`, import `APT_SIGNING_KEY`, run `scripts/build-repo.sh`, upload the site with `actions/upload-pages-artifact` |
| `test` | A `debian:13` container. Serve the built site with `python3 -m http.server`, add it as a source with the published key, `apt-get update`, install all four packages, check `apt-cache policy` shows the expected versions |
| `deploy` | `actions/deploy-pages` in the `github-pages` environment (which GitHub creates when the Pages source is GitHub Actions), only after `test` passes, with `concurrency: pages` so two runs never deploy at once |

A Pages deployment replaces the whole site at once, so clients never see a `Release` that refers to `Packages` files that aren't there yet. A failed build or test leaves the previous site in place.

## Phase 4: Client install and docs

The install instructions, for `README.md`, each app's `README.md`, and `index.html`:

```bash
sudo install -d -m 0755 /etc/apt/keyrings
sudo curl -fsSLo /etc/apt/keyrings/laan.dev.gpg https://apt.laan.dev/laan.dev.gpg
gpg --show-keys /etc/apt/keyrings/laan.dev.gpg   # compare with the fingerprint below
sudo curl -fsSLo /etc/apt/sources.list.d/laan.dev.sources https://apt.laan.dev/laan.dev.sources
sudo apt update
sudo apt install noteliner          # or threadliner, pageliner, blockliner
```

`laan.dev.sources`:

```
Types: deb
URIs: https://apt.laan.dev/
Suites: stable
Components: main
Architectures: amd64
Signed-By: /etc/apt/keyrings/laan.dev.gpg
```

`Architectures: amd64` stops apt on a multiarch system asking for an `i386` index that doesn't exist. Add `arm64` when arm64 packages exist.

The install section states the requirements: Debian 13 or newer, amd64.

Doc updates in `laankit`:

| File | Change |
|---|---|
| `README.md` | An "Install on Debian and Ubuntu" section with the commands above |
| `docs/packaging.md` | The APT repository in the CI section, the Linux row of the signing table marked done, Debian 13 as the minimum for the `.deb` packages, and the per-app tags |
| `SECURITY.md` | The repository key fingerprint, where to report a compromised key, and the rotation steps |
| `ARCHITECTURE.md` | A line on the release path, if it describes releases |

### Update check

The apps' update check (`laankit_core::updates`) points to a GitHub release page. For a package installed from APT, the About dialog could say "Update with `sudo apt upgrade`" instead. One way: the `.deb` installs a marker file (`/usr/share/<pkg>/installed-from-deb`), and the About dialog checks for it. Optional, and independent of the rest.

## Phase 5: End-to-end validation

| Check | How |
|---|---|
| Clean install | A fresh Debian 13 VM: the phase 4 commands, then launch each app from the desktop menu |
| Unsupported release | On Debian 12, `apt install` refuses each package with an unmet `libc6 (>= 2.39)` dependency rather than installing a binary that won't start |
| Per-app release | Releasing one app (`blockliner-v2.6.6`) leaves the other three packages' versions in the repository unchanged |
| Key verification | The fingerprint from `gpg --show-keys` matches `index.html`, `README.md`, and `SECURITY.md` |
| Upgrade | Publish a patch version of one app, wait for the deploy, then `apt update` lists it as upgradable and `apt upgrade` installs it |
| Downgrade | `apt install <pkg>=<older version>` works for the retained versions |
| Tampering | Edit a served `Packages` file locally (a local copy of the site): `apt update` refuses it with a hash mismatch |
| Failed build | A run with a broken signing secret fails at `gpgv` and leaves the live site unchanged |
| Missed dispatch | Publish a release with the dispatch token revoked: the weekly run picks it up |
| Dependency change | A version that adds a dependency installs it on upgrade |
| Removal | `apt purge <pkg>` removes the package and leaves user data (`~/.config`, `~/.local/share`) in place, as expected for a desktop app |
| Upgrade from Tauri builds | On a machine with `note-liner` installed, `apt install noteliner` replaces it (the existing `Conflicts`/`Replaces`/`Provides`) |

## Phase 6 (later): arm64 and key rotation

- arm64: add `ubuntu-24.04-arm` with `formats: deb rpm` to the `release-native.yml` matrix, add `arm64` to `conf/release.conf` and `laan.dev.sources`, and write `binary-arm64/Packages` (`apt-ftparchive packages` with `--arch arm64`, or one `pool/` scan filtered by architecture).
- Routine subkey renewal: extend the subkey's expiry in your keystore (this needs the primary key), re-export the public key to `static/`, and update the CI secret. Clients pick up the new expiry only when they download the key again, so renew at least 3 months early and say so on `index.html`.
- Key compromise: create a new key, sign `Release` with both keys for a transition period (apt accepts a file signed by any key in `Signed-By`, so clients holding either key keep working), publish the new key, then drop the old signature after the transition. Document this in `SECURITY.md`.

## Limits

| Limit | Value (GitHub Pages) | Expected use |
|---|---|---|
| Published site size | 1 GB | About 130 MB at 3 versions of 4 packages, amd64 only; about double with arm64 |
| Bandwidth | 100 GB per month (soft) | About 2,000 to 5,000 full downloads a month; fine for a personal project, and the point to move to object storage with a CDN if exceeded |
| Builds | 10 per hour (soft) | One per release |
| File size | 100 MB per file | The largest `.deb` is NoteLiner at about 18 MB |

## Open questions

- Should the `.rpm` packages get a matching DNF repository (`createrepo_c`, the same key and Pages site under `/rpm/`)? Not part of this plan.
- Should the repository index page link to the AppImage and Flatpak options for other distributions?
