#!/usr/bin/env bash
set -euo pipefail

: "${REPOSITORY:?}"
: "${COMMIT_SHA:?}"
: "${NIGHTLY_VERSION:?}"

if [[ ! "${NIGHTLY_VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+-nightly\.[0-9]+$ ]]; then
  echo "[nightly_release] Invalid nightly version: ${NIGHTLY_VERSION}" >&2
  exit 1
fi

current_tag="tauri-nightly-${NIGHTLY_VERSION}"
tag_refs="$(gh api --paginate "repos/${REPOSITORY}/git/matching-refs/tags/tauri-nightly" --jq '.[].ref')"
release_tags="$(gh api --paginate "repos/${REPOSITORY}/releases?per_page=100" --jq '.[].tag_name')"

if ! grep -Fxq -- "refs/tags/${current_tag}" <<< "${tag_refs}"; then
  echo "[nightly_release] Current nightly tag is missing; refusing cleanup." >&2
  exit 1
fi
if ! grep -Fxq -- "${current_tag}" <<< "${release_tags}"; then
  echo "[nightly_release] Current nightly release is missing; refusing cleanup." >&2
  exit 1
fi

branch_sha="$(gh api "repos/${REPOSITORY}/branches/publish-tauri" --jq '.commit.sha')"
if [[ "${branch_sha}" != "${COMMIT_SHA}" ]]; then
  echo "[nightly_release] The publish branch advanced; skipping cleanup."
  exit 0
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
  echo "[nightly_release] Removed outdated nightly tag: ${tag}"
done <<< "${tag_refs}"

if grep -Fxq -- "tauri-nightly" <<< "${release_tags}"; then
  gh release delete tauri-nightly --repo "${REPOSITORY}" --cleanup-tag --yes
  echo "[nightly_release] Removed the rolling nightly release and tag."
elif grep -Fxq -- "refs/tags/tauri-nightly" <<< "${tag_refs}"; then
  gh api --method DELETE "repos/${REPOSITORY}/git/refs/tags/tauri-nightly" >/dev/null
  echo "[nightly_release] Removed the rolling nightly tag."
fi

echo "[nightly_release] Removed ${removed} outdated nightly releases."
