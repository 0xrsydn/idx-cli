#!/usr/bin/env bash
# CHANGELOG.md and the entries waiting for the next release.
#
# A pull request that changes what users or agents see adds one entry file,
# changes/unreleased/<branch-name>.md (see changes/README.md). At release time
# the entries are folded into CHANGELOG.md, and the release workflow publishes
# that version's section as the GitHub release notes.
#
#   scripts/changelog.sh check                     # validate entries and CHANGELOG.md
#   scripts/changelog.sh release 0.5.0 [YYYY-MM-DD] # fold entries into a new section
#   scripts/changelog.sh notes 0.5.0               # print a version's release notes
#
# CHANGELOG_ROOT points at another checkout (default: this repository).
set -Eeuo pipefail

root="${CHANGELOG_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
changelog="${root}/CHANGELOG.md"
unreleased="${root}/changes/unreleased"
repo_url="https://github.com/0xrsydn/idx-cli"

# Sections in the order a release lists them.
sections=(
  "Security"
  "New Features"
  "Bug Fixes"
  "Improvements"
  "Output Changes"
  "Docs"
  "Testing"
  "Notes"
)

die() {
  printf 'changelog: %s\n' "$*" >&2
  exit 1
}

usage() {
  sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# Entry files in the order a release lists them: by the lowest PR number an
# entry cites, so the release reads in the order the work landed, then by
# name. The summary is left out; it opens the release instead.
entries() {
  [[ -d "$unreleased" ]] || return 0
  local file pr
  find "$unreleased" -maxdepth 1 -type f -name '*.md' ! -name 'summary.md' |
    while IFS= read -r file; do
      pr="$(grep -o '(#[0-9][0-9]*' "$file" | tr -d '(#' | sort -n | head -1 || true)"
      printf '%09d\t%s\n' "$((10#${pr:-999999999}))" "$file"
    done | LC_ALL=C sort | cut -f2-
}

section_names() {
  local IFS='|'
  printf '%s' "${sections[*]}"
}

check_entry() {
  local file="$1"
  awk -v file="$file" -v names="$(section_names)" '
    BEGIN { n = split(names, list, "|"); for (i = 1; i <= n; i++) allowed[list[i]] = 1 }
    /^```/ { fence = !fence }
    fence { if (current != "") body[current]++; next }
    /^#/ {
      if ($0 !~ /^### /) { printf "%s:%d: use \"### <section>\" headings only\n", file, NR; bad = 1; next }
      current = substr($0, 5)
      if (!(current in allowed)) {
        printf "%s:%d: unknown section \"%s\" (use one of: %s)\n", file, NR, current, names; bad = 1
      }
      if (current in seen) { printf "%s:%d: section \"%s\" appears twice\n", file, NR, current; bad = 1 }
      seen[current] = 1; order[++count] = current
      next
    }
    /[^[:space:]]/ {
      if (current == "") { printf "%s:%d: text before the first \"### <section>\" heading\n", file, NR; bad = 1; next }
      body[current]++
    }
    END {
      if (count == 0) { printf "%s: no \"### <section>\" heading\n", file; bad = 1 }
      for (i = 1; i <= count; i++) if (!(order[i] in body)) {
        printf "%s: section \"%s\" is empty\n", file, order[i]; bad = 1
      }
      exit bad
    }
  ' "$file" >&2
}

check_summary() {
  local file="$1"
  awk -v file="$file" '
    /^```/ { fence = !fence }
    !fence && /^#/ { printf "%s:%d: the summary takes no headings\n", file, NR; bad = 1 }
    END { exit bad }
  ' "$file" >&2
}

check_changelog() {
  [[ -f "$changelog" ]] || die "missing ${changelog}"
  awk -v file="$changelog" '
    /^```/ { fence = !fence }
    fence || !/^## / { next }
    {
      if ($0 !~ /^## \[[0-9]+\.[0-9]+\.[0-9]+\] - [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/) {
        printf "%s:%d: version headings read \"## [X.Y.Z] - YYYY-MM-DD\"\n", file, NR; bad = 1; next
      }
      version = substr($2, 2, length($2) - 2)
      if (version in seen) { printf "%s:%d: %s appears twice\n", file, NR, version; bad = 1 }
      seen[version] = 1
      split(version, v, ".")
      key = sprintf("%06d%06d%06d", v[1], v[2], v[3])
      if (last != "" && key >= last) {
        printf "%s:%d: %s is not older than the version above it\n", file, NR, version; bad = 1
      }
      last = key
    }
    END { exit bad }
  ' "$changelog" >&2
}

cmd_check() {
  local failed=0 file
  check_changelog || failed=1
  while IFS= read -r file; do
    check_entry "$file" || failed=1
  done < <(entries)
  if [[ -f "${unreleased}/summary.md" ]]; then
    check_summary "${unreleased}/summary.md" || failed=1
  fi
  ((failed == 0)) || exit 1
  printf 'changelog: ok (%s unreleased entries)\n' "$(entries | wc -l | tr -d ' ')"
}

# A version as a string that sorts like the version: 0.10.0 after 0.9.9.
version_key() {
  local major minor patch
  IFS=. read -r major minor patch <<<"$1"
  printf '%06d%06d%06d' "$((10#$major))" "$((10#$minor))" "$((10#$patch))"
}

# The newest version in CHANGELOG.md, if any.
latest_version() {
  awk '/^## \[[0-9]/ { print substr($2, 2, length($2) - 2); exit }' "$changelog"
}

# Drops leading and trailing blank lines.
trim() {
  awk '
    { lines[++n] = $0 }
    END {
      first = 1; while (first <= n && lines[first] ~ /^[[:space:]]*$/) first++
      last = n; while (last >= first && lines[last] ~ /^[[:space:]]*$/) last--
      for (i = first; i <= last; i++) print lines[i]
    }
  '
}

# One section's lines across every entry, in entry order, each entry's part
# trimmed so the bullets read as one list.
section_body() {
  local name="$1"
  shift
  awk -v want="$name" '
    function flush(   first, last, i) {
      first = 1; while (first <= n && buf[first] ~ /^[[:space:]]*$/) first++
      last = n; while (last >= first && buf[last] ~ /^[[:space:]]*$/) last--
      for (i = first; i <= last; i++) print buf[i]
      n = 0
    }
    FNR == 1 { flush(); keep = 0; fence = 0 }
    {
      if ($0 ~ /^```/) { fence = !fence; if (keep) buf[++n] = $0; next }
      if (!fence && $0 ~ /^### /) { flush(); keep = (substr($0, 5) == want); next }
      if (keep) buf[++n] = $0
    }
    END { flush() }
  ' "$@"
}

cmd_release() {
  local version="${1:-}" date="${2:-}"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "release takes a version such as 0.5.0"
  if [[ -z "$date" ]]; then
    date="$(date -u +%Y-%m-%d)"
  fi
  [[ "$date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || die "the date reads YYYY-MM-DD"
  cmd_check >/dev/null
  if grep -q "^## \[${version//./\\.}\] " "$changelog"; then
    die "${version} is already in CHANGELOG.md"
  fi
  # The new section goes on top, so it must be the newest version; checked
  # before anything is written or an entry deleted.
  local previous
  previous="$(latest_version)"
  if [[ -n "$previous" ]] && [[ "$(version_key "$version")" < "$(version_key "$previous")" ]]; then
    die "${version} is older than ${previous}, the newest version in CHANGELOG.md"
  fi

  local files=()
  local file
  while IFS= read -r file; do
    files+=("$file")
  done < <(entries)
  ((${#files[@]} > 0)) || die "no entries in changes/unreleased/ to release"

  local section body
  local out
  out="$(mktemp "${TMPDIR:-/tmp}/changelog.XXXXXX")"
  {
    printf '## [%s] - %s\n\n' "$version" "$date"
    if [[ -f "${unreleased}/summary.md" ]]; then
      trim <"${unreleased}/summary.md"
      printf '\n'
    fi
    for section in "${sections[@]}"; do
      body="$(section_body "$section" "${files[@]}")"
      [[ -n "$body" ]] || continue
      printf '### %s\n\n%s\n\n' "$section" "$body"
    done
    if [[ -n "$previous" ]]; then
      printf '**Full Changelog**: %s/compare/v%s...v%s\n\n' "$repo_url" "$previous" "$version"
    fi
  } >"$out"

  # The new section goes above the newest version (or at the end).
  awk -v section="$out" '
    !done && /^## \[/ { while ((getline line < section) > 0) print line; done = 1 }
    { print }
    END { if (!done) { print ""; while ((getline line < section) > 0) print line } }
  ' "$changelog" >"${out}.full"
  mv "${out}.full" "$changelog"
  rm -f "$out" "${files[@]}" "${unreleased}/summary.md"
  printf 'changelog: %s released from %d entries; review CHANGELOG.md, then commit\n' \
    "$version" "${#files[@]}"
}

# A version's section without its heading, headings raised one level to
# match GitHub release notes (### New Features -> ## New Features).
cmd_notes() {
  local version="${1:-}"
  version="${version#v}"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "notes takes a version such as 0.5.0"
  [[ -f "$changelog" ]] || die "missing ${changelog}"
  awk -v want="$version" '
    /^```/ && inside { fence = !fence }
    !fence && /^## \[/ {
      if (inside) exit
      if (substr($2, 2, length($2) - 2) == want) { inside = 1; found = 1; next }
    }
    inside {
      if (!fence && /^###/) sub(/^#/, "")
      lines[++n] = $0
    }
    END {
      if (!found) exit 3
      first = 1; while (first <= n && lines[first] ~ /^[[:space:]]*$/) first++
      last = n; while (last >= first && lines[last] ~ /^[[:space:]]*$/) last--
      for (i = first; i <= last; i++) print lines[i]
    }
  ' "$changelog" || {
    local status=$?
    ((status == 3)) && die "${version} is not in CHANGELOG.md"
    exit "$status"
  }
}

case "${1:-}" in
  check) shift; cmd_check "$@" ;;
  release) shift; cmd_release "$@" ;;
  notes) shift; cmd_notes "$@" ;;
  -h | --help | help) usage ;;
  *) usage >&2; exit 2 ;;
esac
