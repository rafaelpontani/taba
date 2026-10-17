# Security policy

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through
[GitHub Security Advisories](https://github.com/rafaelpontani/taba/security/advisories/new). You will get an answer
within 7 days; the fix is published as a new version and the advisory is disclosed once it is available.

## Supported versions

Only the latest final version of each `0.x` line (the `v0.1` floating tag, for example) receives fixes. Release
candidates (`-rc.N`) are for testing and are not supported.

## How releases are produced

Every version is built, tested and published only by [`.github/workflows/release.yml`](.github/workflows/release.yml),
from the commit of its tag. Images, binaries and packages carry SLSA provenance and Sigstore signatures; see
[Verifying a release](README.md#verifying-a-release) for the commands to check them yourself.
