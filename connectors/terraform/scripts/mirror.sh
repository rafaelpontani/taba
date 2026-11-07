#!/usr/bin/env bash
# Publishes a version of the provider to the mirror repository the Terraform and OpenTofu registries read (D211): the
# source of connectors/terraform (with connectors/go in go/, so the mirror builds by itself), a commit and the tag
# v<version> on its main branch, and a GitHub Release with the signed files of scripts/package.go. Run by release.yml;
# idempotent (a commit, tag or Release that already exists is kept, and the files are checked against it).
#   connectors/terraform/scripts/mirror.sh <version> <folder with the signed files> <owner/repository>
# Needs GH_TOKEN with contents:write on the mirror, git and gh. Writes nothing to this repository.
set -euo pipefail
version="${1:?usage: mirror.sh <version> <folder> <owner/repository>}"
dist="${2:?usage: mirror.sh <version> <folder> <owner/repository>}"
repo="${3:?usage: mirror.sh <version> <folder> <owner/repository>}"
: "${GH_TOKEN:?GH_TOKEN must have contents:write on $repo}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
dist="$(cd "$dist" && pwd)"
tag="v$version"
sums="terraform-provider-taba_${version}_SHA256SUMS"
for f in "$sums" "$sums.sig" "terraform-provider-taba_${version}_manifest.json"; do
  [ -f "$dist/$f" ] || { echo "missing $dist/$f" >&2; exit 1; }
done
source_sha="$(git -C "$root" rev-parse HEAD)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
git clone --quiet "https://x-access-token:${GH_TOKEN}@github.com/${repo}.git" "$work/mirror"
cd "$work/mirror"
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "tag $tag already exists in $repo: the source is kept"
else
  # The whole tree is replaced: what left connectors/terraform leaves the mirror too.
  find . -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
  (cd "$root/connectors/terraform" && git ls-files -z | xargs -0 cp --parents -t "$work/mirror")
  mkdir go
  (cd "$root/connectors/go" && git ls-files -z | xargs -0 cp --parents -t "$work/mirror/go")
  cp "$root/LICENSE" .
  sed -i 's#=> \.\./go$#=> ./go#' go.mod
  grep -q '=> ./go$' go.mod || { echo "go.mod has no replace of ../go to rewrite" >&2; exit 1; }
  git add -A
  git commit --quiet --allow-empty -m "terraform-provider-taba $tag" \
    -m "Source: https://github.com/rafaelpontani/taba/tree/$source_sha/connectors/terraform"
  git tag -a "$tag" -m "terraform-provider-taba $tag"
  git push --quiet origin HEAD:main "refs/tags/$tag"
  echo "pushed $tag to $repo"
fi

prerelease=()
[[ $version == *-rc.* ]] && prerelease=(--prerelease)
if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  echo "Release $tag already exists in $repo"
else
  gh release create "$tag" --repo "$repo" --verify-tag --title "$tag" ${prerelease[@]+"${prerelease[@]}"} \
    --notes "Taba provider $tag, built from https://github.com/rafaelpontani/taba/commit/$source_sha (connectors/terraform)." \
    "$dist"/terraform-provider-taba_"${version}"_*
fi

# What the registry downloads must be what was signed here.
mkdir "$work/check"
gh release download "$tag" --repo "$repo" --dir "$work/check"
(cd "$work/check" && cmp "$sums" "$dist/$sums" && cmp "$sums.sig" "$dist/$sums.sig" && sha256sum -c "$sums")
