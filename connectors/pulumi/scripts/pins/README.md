# Pinned build dependencies of the Pulumi packages

`scripts/package.sh` installs only what these files list, with the hash of every package, so the packages published to
npm and PyPI are built from known dependencies (the OpenSSF Scorecard `Pinned-Dependencies` check, run on every pull
request by `.github/scripts/pinned-deps.sh`):

| File | Used by | How |
|---|---|---|
| `nodejs/package-lock.json` | the npm package (`sdk/nodejs`) | copied next to the generated `package.json` and installed with `npm ci` |
| `requirements-build.txt` | the wheel and the sdist (`sdk/python`) | `pip install --require-hashes -r`, then `python -m build --no-isolation` |

The lock lives here, not in `sdk/nodejs`, because `generate.sh` empties `sdk/` on every run. `test.sh sdks` (CI job
`connector-pulumi`) runs `npm ci` with it against the generated `package.json` and fails when they no longer match, for
example after `generate.sh` with another Pulumi version.

## Regenerating

From the repository root (Git Bash on Windows: prefix with `MSYS_NO_PATHCONV=1`):

```
# npm: the lock of the generated package.json (the placeholder version is replaced, as package.sh does)
docker run --rm -v "$PWD:/repo" node:22-bookworm bash -euc '
  cp -r /repo/connectors/pulumi/sdk/nodejs /tmp/sdk && cd /tmp/sdk && sed -i "s/\${VERSION}/0.0.0/" package.json
  npm install --package-lock-only --ignore-scripts --no-audit --no-fund
  cp package-lock.json /repo/connectors/pulumi/scripts/pins/nodejs/'

# Python: the input is the line in the header of requirements-build.txt
mkdir -p /tmp/pin && printf "build==1.3.0\nsetuptools==84.2.1\n" > /tmp/pin/build.in
docker run --rm -v /tmp/pin:/w python:3.12-bookworm sh -c "pip install -q pip-tools && pip-compile -q --generate-hashes --allow-unsafe --strip-extras --no-header -o /w/build.txt /w/build.in"
# then replace everything below the header of requirements-build.txt with /tmp/pin/build.txt
```

Then run `scripts/test.sh sdks` and `SDKS="nodejs python" scripts/package.sh 0.0.0-local /tmp/pulumi-pkg`.
