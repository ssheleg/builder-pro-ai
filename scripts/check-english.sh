#!/usr/bin/env bash
# scripts/check-english.sh — the English-only enforcement gate (spec D2, objective O-2).
#
# Scans the enforced tree — src/ crates/ src-tauri/src/ tests/ scripts/ docs/ plus README.md and
# CHANGELOG.md — for Cyrillic (the full Unicode block U+0400..U+04FF, i.e. `[Ѐ-ӿ]`, a superset of
# `[А-Яа-яЁё]`), subtracts the exact paths listed in scripts/english-allowlist.txt (the closed set
# of pre-existing frozen historical records), and FAILS (exit 1) if any offending file remains.
# Prints an OK line and exits 0 when the tree is clean.
#
# Determinism note: the repo's `grep` is often `ugrep`, whose `-P` (PCRE) engine has been observed
# to MISS some Cyrillic-bearing files non-deterministically. This gate therefore does the scan in
# Python (primary) or Perl (fallback) with an explicit codepoint/byte pattern — never `grep -P` —
# so the result is reproducible on every machine and in CI.
#
# Wired into scripts/final-suite.sh (a numbered stage) and .github/workflows/ci.yml (an early
# step). Keep those two in lockstep (CONTRIBUTING.md).
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"
ALLOWLIST="$REPO/scripts/english-allowlist.txt"

if [[ ! -f "$ALLOWLIST" ]]; then
  echo "FAIL: allowlist not found at $ALLOWLIST" >&2
  exit 2
fi

if command -v python3 >/dev/null 2>&1; then
  python3 - "$ALLOWLIST" <<'PY'
import os, re, sys, subprocess

allowlist_path = sys.argv[1]
# Full Cyrillic block U+0400..U+04FF (Ѐ..ӿ) — superset of spec D2's [А-Яа-яЁё].
CYRILLIC = re.compile(r'[Ѐ-ӿ]')
ROOT_DIRS = ['src', 'crates', 'src-tauri/src', 'tests', 'scripts', 'docs']
ROOT_FILES = ['README.md', 'CHANGELOG.md']

# Read the allowlist: one repo-relative path per line; '#' comments and blank lines ignored.
allow = set()
with open(allowlist_path, encoding='utf-8') as fh:
    for raw in fh:
        line = raw.split('#', 1)[0].strip()
        if line:
            allow.add(line)

# Enumerate the enforced set from git's tracked files (deterministic, honors .gitignore, skips
# build junk); fall back to a filesystem walk outside a git checkout.
files = []
try:
    out = subprocess.check_output(
        ['git', 'ls-files', '-z', '--', *ROOT_DIRS, *ROOT_FILES], text=True)
    files = [f for f in out.split('\0') if f]
except Exception:
    for d in ROOT_DIRS:
        for base, _dirs, names in os.walk(d):
            for n in names:
                files.append(os.path.join(base, n))
    files += [f for f in ROOT_FILES if os.path.isfile(f)]

# Non-fatal hygiene: flag allowlist entries that no longer exist so the list can be pruned, but do
# not fail the gate on them (deleting a historical record must not break CI).
existing = set(files)
for a in sorted(allow):
    if a not in existing:
        print(f"note: allowlist entry no longer present (safe to prune): {a}", file=sys.stderr)

offenders = []
for f in sorted(files):
    if f in allow:
        continue
    try:
        with open(f, encoding='utf-8', errors='ignore') as fh:
            data = fh.read()
    except OSError:
        continue
    if CYRILLIC.search(data):
        offenders.append(f)

if offenders:
    print("FAIL: Cyrillic (U+0400..U+04FF) found outside the allowlist — English-only, spec D2 / O-2:")
    for f in offenders:
        print(f"  {f}")
    print()
    print("Fix: translate the file to English. Do NOT add it to scripts/english-allowlist.txt")
    print("unless it is a pre-existing FROZEN historical record (a superseded spec/plan/qa/research")
    print("doc), in which case add its exact path there with a reason.")
    sys.exit(1)

print(f"OK: no Cyrillic outside the allowlist ({len(files)} files scanned, {len(allow)} allowlisted).")
PY
elif command -v perl >/dev/null 2>&1; then
  # Byte-level fallback: UTF-8 for U+0400..U+04FF is exactly the 2-byte range 0xD0-0xD3 followed by
  # a 0x80-0xBF continuation byte, so a raw-byte match needs no decode and never dies on binary.
  perl -e '
    use strict; use warnings;
    my $allow_path = shift;
    my %allow;
    open(my $a, "<", $allow_path) or die "cannot read allowlist: $!\n";
    while (<$a>) { s/#.*//; s/^\s+|\s+$//g; $allow{$_} = 1 if length; }
    close $a;
    my @roots = qw(src crates src-tauri/src tests scripts docs README.md CHANGELOG.md);
    my @files = grep { length } split /\0/, `git ls-files -z -- @roots`;
    my %existing = map { $_ => 1 } @files;
    for my $k (sort keys %allow) {
      warn "note: allowlist entry no longer present (safe to prune): $k\n" unless $existing{$k};
    }
    my @offenders;
    for my $f (sort @files) {
      next if $allow{$f};
      open(my $fh, "<:raw", $f) or next;
      local $/; my $data = <$fh>; close $fh;
      push @offenders, $f if defined $data && $data =~ /[\xD0-\xD3][\x80-\xBF]/;
    }
    if (@offenders) {
      print "FAIL: Cyrillic (U+0400..U+04FF) found outside the allowlist — English-only, spec D2 / O-2:\n";
      print "  $_\n" for @offenders;
      exit 1;
    }
    printf "OK: no Cyrillic outside the allowlist (%d files scanned, %d allowlisted).\n",
      scalar(@files), scalar(keys %allow);
  ' "$ALLOWLIST"
else
  echo "FAIL: neither python3 nor perl is available to run the English-only gate" >&2
  exit 2
fi
