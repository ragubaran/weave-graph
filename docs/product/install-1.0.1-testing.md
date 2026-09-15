# Test-installing Weave Graph 1.0.1

Use this guide to test the `1.0.1` command-line binary before adopting it in a team workflow. Confirm the installed binary reports exactly `weave 1.0.1` before indexing a real repository.

## Homebrew

This path works only after the `weave-graph/tap` formula has been updated to the `v1.0.1` release asset.

```bash
brew update
brew tap weave-graph/tap
brew install weave-graph/tap/weave
weave --version
```

If Homebrew reports that the tap or formula is unavailable, use the release archive below. Do not substitute an unverified third-party formula.

## macOS and Linux release archive

The release pipeline emits `.tar.gz` archives for macOS and Linux, not ZIP files. Select the target that matches your machine:

| Platform | Target |
| --- | --- |
| Apple Silicon macOS | `aarch64-apple-darwin` |
| Intel macOS | `x86_64-apple-darwin` |
| Intel Linux | `x86_64-unknown-linux-musl` |
| ARM64 Linux | `aarch64-unknown-linux-musl` |

Replace `TARGET` below, then download the archive and the release checksum manifest. These commands assume the `v1.0.1` GitHub Release and its assets have been published.

```bash
TARGET=x86_64-apple-darwin
ARCHIVE="weave-v1.0.1-${TARGET}.tar.gz"
BASE="https://github.com/ragubaran/weave-graph/releases/download/v1.0.1"

curl -fLO "$BASE/$ARCHIVE"
curl -fLO "$BASE/SHA256SUMS.txt"
grep "  $ARCHIVE$" SHA256SUMS.txt | shasum -a 256 -c -
tar -xzf "$ARCHIVE"
./weave --version
```

On Linux, replace the checksum command with:

```bash
grep "  $ARCHIVE$" SHA256SUMS.txt | sha256sum -c -
```

After the version check, place the binary on your user path:

```bash
mkdir -p "$HOME/.local/bin"
mv weave "$HOME/.local/bin/weave"
export PATH="$HOME/.local/bin:$PATH"
weave --version
```

## Windows ZIP archive

Windows release assets use ZIP. In PowerShell:

```powershell
$archive = 'weave-v1.0.1-x86_64-pc-windows-msvc.zip'
$base = 'https://github.com/ragubaran/weave-graph/releases/download/v1.0.1'
Invoke-WebRequest "$base/$archive" -OutFile $archive
Invoke-WebRequest "$base/SHA256SUMS.txt" -OutFile SHA256SUMS.txt
Expand-Archive $archive -DestinationPath weave-1.0.1
.\weave-1.0.1\weave.exe --version
```

Compare `Get-FileHash $archive -Algorithm SHA256` with the matching entry in `SHA256SUMS.txt` before placing `weave.exe` on `PATH`.

## Smoke test

Run this in a disposable repository after installation:

```bash
mkdir weave-1.0.1-smoke && cd weave-1.0.1-smoke
git init
printf 'pub fn greet() {}\nfn caller() { greet(); }\n' > lib.rs
weave init --mode single
weave index
weave query 'callers(greet)'
weave report
```

Expected result: indexing succeeds, the query names `caller`, and `weave report` creates files under `.weave/report/`.

For a normal release install, see [Getting Started](getting-started.md).
