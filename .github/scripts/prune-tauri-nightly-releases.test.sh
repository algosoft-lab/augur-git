#!/usr/bin/env bash
set -euo pipefail

fixture_dir="$(mktemp -d)"
trap 'rm -rf "${fixture_dir}"' EXIT
mkdir -p "${fixture_dir}/bin"

cat > "${fixture_dir}/bin/gh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == api && "${2:-}" == "repos/${REPOSITORY}/git/ref/tags/tauri-nightly" ]]; then
  printf '%s\n' "${GH_ROLLING_SHA}"
elif [[ "${1:-}" == api && "${2:-}" == --paginate && "${3:-}" == "repos/${REPOSITORY}/git/matching-refs/tags/tauri-nightly-" ]]; then
  cat "${GH_FIXTURE_DIR}/refs"
elif [[ "${1:-}" == api && "${2:-}" == --paginate && "${3:-}" == "repos/${REPOSITORY}/releases?per_page=100" ]]; then
  cat "${GH_FIXTURE_DIR}/releases"
elif [[ "${1:-}" == release && "${2:-}" == delete ]]; then
  printf 'release:%s\n' "${3}" >> "${GH_LOG_PATH}"
elif [[ "${1:-}" == api && "${2:-}" == --method && "${3:-}" == DELETE ]]; then
  printf 'tag:%s\n' "${4##*/}" >> "${GH_LOG_PATH}"
else
  printf 'Unexpected gh call: %s\n' "$*" >&2
  exit 2
fi
MOCK
chmod +x "${fixture_dir}/bin/gh"

export PATH="${fixture_dir}/bin:${PATH}"
export GH_FIXTURE_DIR="${fixture_dir}"
export GH_LOG_PATH="${fixture_dir}/deletions"
export GH_ROLLING_SHA="current-commit"
export REPOSITORY="algosoft-lab/augur-git"
export COMMIT_SHA="current-commit"
export NIGHTLY_VERSION="0.1.1-nightly.16"

cat > "${fixture_dir}/refs" <<'REFS'
refs/tags/tauri-nightly-0.1.1-nightly.15
refs/tags/tauri-nightly-0.1.1-nightly.16
refs/tags/tauri-nightly-0.1.1-nightly.14
refs/tags/tauri-nightly-0.1.1-nightly.16-extra
refs/tags/tauri-nightly-unrelated
REFS
cat > "${fixture_dir}/releases" <<'RELEASES'
tauri-nightly-0.1.1-nightly.15
tauri-nightly-0.1.1-nightly.16
RELEASES

bash .github/scripts/prune-tauri-nightly-releases.sh >/dev/null
expected=$'release:tauri-nightly-0.1.1-nightly.15\ntag:tauri-nightly-0.1.1-nightly.14'
if [[ "$(cat "${GH_LOG_PATH}")" != "${expected}" ]]; then
  echo "Cleanup selected the wrong tags or deletion method." >&2
  exit 1
fi

: > "${GH_LOG_PATH}"
GH_ROLLING_SHA="another-commit" bash .github/scripts/prune-tauri-nightly-releases.sh >/dev/null
if [[ -s "${GH_LOG_PATH}" ]]; then
  echo "Cleanup deleted tags after the rolling tag moved." >&2
  exit 1
fi

: > "${GH_LOG_PATH}"
printf '%s\n' "refs/tags/tauri-nightly-0.1.1-nightly.15" > "${fixture_dir}/refs"
if bash .github/scripts/prune-tauri-nightly-releases.sh >/dev/null 2>&1; then
  echo "Cleanup accepted a missing current nightly tag." >&2
  exit 1
fi
if [[ -s "${GH_LOG_PATH}" ]]; then
  echo "Cleanup deleted tags without a current nightly tag." >&2
  exit 1
fi

: > "${GH_LOG_PATH}"
printf '%s\n' "refs/tags/tauri-nightly-0.1.1-nightly.16" >> "${fixture_dir}/refs"
printf '%s\n' "tauri-nightly-0.1.1-nightly.15" > "${fixture_dir}/releases"
if bash .github/scripts/prune-tauri-nightly-releases.sh >/dev/null 2>&1; then
  echo "Cleanup accepted a missing current nightly release." >&2
  exit 1
fi
if [[ -s "${GH_LOG_PATH}" ]]; then
  echo "Cleanup deleted tags without a current nightly release." >&2
  exit 1
fi

echo "Nightly release cleanup tests passed."
