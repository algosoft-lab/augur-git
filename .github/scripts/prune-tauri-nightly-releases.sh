#!/usr/bin/env bash
set -euo pipefail

: "${REPOSITORY:?}"
: "${COMMIT_SHA:?}"
: "${NIGHTLY_VERSION:?}"

if [[ ! "${NIGHTLY_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+-nightly\.[0-9]+$ ]]; then
  echo "Invalid nightly version: ${NIGHTLY_VERSION}" >&2
  exit 1
fi

rolling_sha="$(gh api "repos/${REPOSITORY}/git/ref/tags/tauri-nightly" --jq '.object.sha')"
if [[ "${rolling_sha}" != "${COMMIT_SHA}" ]]; then
  echo "The rolling tag no longer points to this build; skipping cleanup."
  exit 0
fi

current_tag="tauri-nightly-${NIGHTLY_VERSION}"
tag_refs="$(gh api --paginate "repos/${REPOSITORY}/git/matching-refs/tags/tauri-nightly-" --jq '.[].ref')"
release_tags="$(gh api --paginate "repos/${REPOSITORY}/releases?per_page=100" --jq '.[].tag_name')"

if ! grep -Fxq -- "refs/tags/${current_tag}" <<< "${tag_refs}"; then
  echo "Current nightly tag is missing; refusing to delete older tags." >&2
  exit 1
fi
if ! grep -Fxq -- "${current_tag}" <<< "${release_tags}"; then
  echo "Current nightly release is missing; refusing to delete older releases." >&2
  exit 1
fi

removed=0
while IFS= read -r ref; do
  tag="${ref#refs/tags/}"
  if [[ ! "${tag}" =~ ^tauri-nightly-[0-9]+\.[0-9]+\.[0-9]+-nightly\.[0-9]+$ ]]; then
    continue
  fi
  if [[ "${tag}" == "${current_tag}" ]]; then
    continue
  fi

  if grep -Fxq -- "${tag}" <<< "${release_tags}"; then
    gh release delete "${tag}" --repo "${REPOSITORY}" --cleanup-tag --yes
  else
    gh api --method DELETE "repos/${REPOSITORY}/git/refs/tags/${tag}" >/dev/null
  fi
  removed=$((removed + 1))
  echo "Removed outdated Tauri nightly tag: ${tag}"
done <<< "${tag_refs}"

echo "Removed ${removed} outdated Tauri nightly tags."
