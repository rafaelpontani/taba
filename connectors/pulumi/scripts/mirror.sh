#!/usr/bin/env bash
# Publishes a version of the Pulumi provider to the mirror repository (D213, pulumi-taba): the Go SDK module (sdk/), the schema, the
# examples, the README, the licence, a commit on its main branch, the tags v<version> and sdk/v<version> (the Go module is in
# the sdk/ folder, so Go reads its versions from the sdk/ tags), and a GitHub Release with the files of package.sh
# (the plugin archives that `pulumi plugin install resource taba <version> --server github://api.github.com/<owner>/pulumi-taba`
# downloads, the packages and the SHA256SUMS). Run by release.yml; idempotent (a commit, tag or Release that already
# exists is kept, and the files are checked against it).
#   connectors/pulumi/scripts/mirror.sh <version> <folder with the files of package.sh> <owner/repository>
# Needs GH_TOKEN with contents:write on the mirror, git and gh. Writes nothing to this repository.
set -euo pipefail
version="${1:?usage: mirror.sh <version> <folder> <owner/repository>}"
dist="${2:?usage: mirror.sh <version> <folder> <owner/repository>}"
repo="${3:?usage: mirror.sh <version> <folder> <owner/repository>}"
: "${GH_TOKEN:?GH_TOKEN must have contents:write on $repo}"
# The GitHub API answers a 5xx now and then (the v0.3.11-rc.1 download of an asset got a 500 and the job failed with the
# mirror already complete): what can be repeated is repeated, with a pause that grows.
retry() {
  local attempt=1
  until "$@"; do
    [ "$attempt" -lt 5 ] || { echo "failed after $attempt attempts: $*" >&2; return 1; }
    echo "attempt $attempt failed, trying again: $*" >&2
    sleep $((attempt * 10))
    attempt=$((attempt + 1))
  done
}
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
dist="$(cd "$dist" && pwd)"
tag="v$version"
sdk_tag="sdk/v$version"
sums="pulumi-taba_${version}_SHA256SUMS"
[ -f "$dist/$sums" ] || { echo "missing $dist/$sums" >&2; exit 1; }
source_sha="$(git -C "$root" rev-parse HEAD)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
retry git clone --quiet "https://x-access-token:${GH_TOKEN}@github.com/${repo}.git" "$work/mirror"
cd "$work/mirror"
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "tag $tag already exists in $repo: the source is kept"
  git rev-parse -q --verify "refs/tags/$sdk_tag" >/dev/null || { echo "$repo has $tag but not $sdk_tag" >&2; exit 1; }
else
  # The whole tree is replaced: what left connectors/pulumi leaves the mirror too.
  find . -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
  (cd "$root/connectors/pulumi" && git ls-files -z -- sdk examples provider/cmd/pulumi-resource-taba/schema.json | xargs -0 cp --parents -t "$work/mirror")
  cp "$root/connectors/LICENSE" LICENSE
  cp "$root/connectors/pulumi/README.md" .
  [ -f sdk/go.mod ] || { echo "sdk/go.mod is not in the source" >&2; exit 1; }
  git add -A
  git commit --quiet --allow-empty -m "pulumi-taba $tag" \
    -m "Source: https://github.com/rafaelpontani/taba/tree/$source_sha/connectors/pulumi"
  git tag -a "$tag" -m "pulumi-taba $tag"
  git tag -a "$sdk_tag" -m "pulumi-taba sdk $tag"
  retry git push --quiet origin HEAD:main "refs/tags/$tag" "refs/tags/$sdk_tag"
  echo "pushed $tag and $sdk_tag to $repo"
fi

prerelease=()
[[ $version == *-rc.* ]] && prerelease=(--prerelease)
if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  echo "Release $tag already exists in $repo"
else
  # A creation that failed half way (the Release is there, some files are not) is completed by the next attempt.
  create_release() {
    if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
      gh release upload "$tag" --repo "$repo" --clobber "$dist"/*
    else
      gh release create "$tag" --repo "$repo" --verify-tag --title "$tag" ${prerelease[@]+"${prerelease[@]}"} \
        --notes "Taba provider for Pulumi $tag, built from https://github.com/rafaelpontani/taba/commit/$source_sha (connectors/pulumi)." \
        "$dist"/*
    fi
  }
  retry create_release
fi

# What `pulumi plugin install` downloads must be what was checked here.
mkdir "$work/check"
retry gh release download "$tag" --repo "$repo" --dir "$work/check"
(cd "$work/check" && cmp "$sums" "$dist/$sums" && sha256sum -c "$sums")
