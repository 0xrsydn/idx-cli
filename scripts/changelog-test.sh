#!/usr/bin/env bash
# Tests scripts/changelog.sh against a scratch checkout, then checks the real
# CHANGELOG.md and entries.
set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script="${repo_root}/scripts/changelog.sh"
work="$(mktemp -d "${TMPDIR:-/tmp}/idx-changelog-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT
failures=0

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  failures=$((failures + 1))
}

changelog() {
  CHANGELOG_ROOT="$work" bash "$script" "$@"
}

# A checkout with one released version and no entries.
reset() {
  rm -rf "${work:?}"/*
  mkdir -p "$work/changes/unreleased"
  cat >"$work/CHANGELOG.md" <<'EOF'
# Changelog

Intro.

## [0.4.0] - 2026-10-19

### Bug Fixes

- An old fix. (#1)
EOF
}

entry() {
  cat >"$work/changes/unreleased/$1"
}

expect_check_error() {
  local what="$1" pattern="$2" err
  if err="$(changelog check 2>&1)"; then
    fail "check accepted ${what}"
  elif ! grep -q -- "$pattern" <<<"$err"; then
    fail "check on ${what} said: ${err}"
  fi
}

# --- check -----------------------------------------------------------------

reset
changelog check >/dev/null || fail "check refused a clean checkout"

entry bad.md <<'EOF'
### Fixes

- x (#2)
EOF
expect_check_error "an unknown section" 'unknown section "Fixes"'

reset
entry bad.md <<'EOF'
- a bullet before any heading (#2)

### Bug Fixes

- x (#2)
EOF
expect_check_error "text before a heading" "text before the first"

reset
entry bad.md <<'EOF'
### Bug Fixes

### New Features

- x (#2)
EOF
expect_check_error "an empty section" 'section "Bug Fixes" is empty'

reset
entry bad.md <<'EOF'
## Bug Fixes

- x (#2)
EOF
expect_check_error "a level-2 heading" 'use "### <section>" headings only'

reset
entry summary.md <<'EOF'
### Not here
EOF
expect_check_error "a heading in the summary" "the summary takes no headings"

reset
printf '\n## [0.5.0] - 2026-11-05\n\n- newer below older\n' >>"$work/CHANGELOG.md"
expect_check_error "versions out of order" "is not older than the version above it"

reset
printf '\n## 0.3.0\n' >>"$work/CHANGELOG.md"
expect_check_error "a malformed version heading" 'version headings read'

reset
printf '\n## [0.03.0] - 2026-01-01\n' >>"$work/CHANGELOG.md"
expect_check_error "a version with a leading zero" 'version headings read'

# --- release -----------------------------------------------------------------

# b-feature.md cites a lower PR than a-fix.md, so it comes first.
reset
entry b-feature.md <<'EOF'
### New Features

- Feature B, which spans
  two lines. (#9)

### Output Changes

- **Breaking:** field `x` is gone. (#9)
EOF
entry a-fix.md <<'EOF'
### Bug Fixes

- Fix A. (#10)

### New Features

- Feature A, with an example:

  ```bash
  # a comment, not a heading
  idx stocks quote BBCA
  ```
EOF
entry summary.md <<'EOF'

**Upgrade from 0.4.0**: it fixes A.

EOF
changelog release 0.5.0 2026-11-05 >/dev/null || fail "release failed"

expected="$work/expected.md"
cat >"$expected" <<'EOF'
# Changelog

Intro.

## [0.5.0] - 2026-11-05

**Upgrade from 0.4.0**: it fixes A.

### New Features

- Feature B, which spans
  two lines. (#9)
- Feature A, with an example:

  ```bash
  # a comment, not a heading
  idx stocks quote BBCA
  ```

### Bug Fixes

- Fix A. (#10)

### Output Changes

- **Breaking:** field `x` is gone. (#9)

**Full Changelog**: https://github.com/0xrsydn/idx-cli/compare/v0.4.0...v0.5.0

## [0.4.0] - 2026-10-19

### Bug Fixes

- An old fix. (#1)
EOF
diff -u "$expected" "$work/CHANGELOG.md" || fail "release wrote a different CHANGELOG.md"
if find "$work/changes/unreleased" -name '*.md' | grep -q .; then
  fail "release left entries behind"
fi
changelog check >/dev/null || fail "check refused the released CHANGELOG.md"

entry again.md <<'EOF'
### Bug Fixes

- x (#12)
EOF
if changelog release 0.5.0 2026-11-06 >/dev/null 2>&1; then
  fail "release accepted a version already released"
fi
if err="$(changelog release 0.4.9 2026-11-06 2>&1)"; then
  fail "release accepted a version older than the newest"
elif ! grep -q "older than 0.5.0" <<<"$err"; then
  fail "release of an older version said: ${err}"
fi
if changelog release 00.5.0 2026-11-06 >/dev/null 2>&1; then
  fail "release accepted a version with a leading zero"
fi
[[ -f "$work/changes/unreleased/again.md" ]] || fail "a refused release deleted the entries"
diff -q "$expected" "$work/CHANGELOG.md" >/dev/null || fail "a refused release changed CHANGELOG.md"
rm "$work/changes/unreleased/again.md"
if changelog release 0.5.1 >/dev/null 2>&1; then
  fail "release accepted no entries"
fi

# --- notes -------------------------------------------------------------------

notes="$(changelog notes v0.5.0)"
[[ "$(head -1 <<<"$notes")" == "**Upgrade from 0.4.0**: it fixes A." ]] ||
  fail "notes do not open with the summary: $(head -1 <<<"$notes")"
grep -qx '## New Features' <<<"$notes" || fail "notes did not raise section headings"
grep -qx '  # a comment, not a heading' <<<"$notes" || fail "notes changed a code block"
grep -q '0.4.0\] -' <<<"$notes" && fail "notes ran into the next version"
[[ "$(tail -1 <<<"$notes")" == "**Full Changelog**: https://github.com/0xrsydn/idx-cli/compare/v0.4.0...v0.5.0" ]] ||
  fail "notes do not end with the compare link"
[[ "$(changelog notes 0.4.0)" == "## Bug Fixes"$'\n\n'"- An old fix. (#1)" ]] ||
  fail "notes for the oldest version: $(changelog notes 0.4.0)"
if changelog notes 0.9.9 >/dev/null 2>&1; then
  fail "notes accepted a version not in CHANGELOG.md"
fi

# --- the real changelog --------------------------------------------------------

bash "$script" check >/dev/null || fail "the repository's changelog entries do not pass check"
while read -r version; do
  [[ -n "$(bash "$script" notes "$version")" ]] || fail "no notes for ${version}"
done < <(awk '/^## \[[0-9]/ { print substr($2, 2, length($2) - 2) }' "${repo_root}/CHANGELOG.md")

if ((failures > 0)); then
  printf '%d changelog test(s) failed\n' "$failures" >&2
  exit 1
fi
printf 'changelog tests passed\n'
