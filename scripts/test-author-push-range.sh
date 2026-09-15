#!/usr/bin/env bash
# A valid push must not inherit historical identity failures,
# but a newly introduced unauthorized identity must still fail.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT
mkdir -p "$tmp/scripts"
cp "$root/scripts/check-author-policy.sh" "$tmp/scripts/"
git -C "$tmp" init -q
git -C "$tmp" add scripts/check-author-policy.sh
git -C "$tmp" -c user.name="Historical fixture" -c user.email=fixture@example.invalid commit -qm baseline
before="$(git -C "$tmp" rev-parse HEAD)"
git -C "$tmp" -c user.name="ZEKRITI Tarek" -c user.email=contact@checkupauto.fr commit --allow-empty -qm valid
good="$(git -C "$tmp" rev-parse HEAD)"
bash "$tmp/scripts/check-author-policy.sh" "$before..$good"
if bash "$tmp/scripts/check-author-policy.sh" "$good" > "$tmp/historical.log" 2>&1; then
  echo "full-history fixture unexpectedly passed" >&2; exit 1
fi
git -C "$tmp" -c user.name="Unauthorized fixture" -c user.email=fixture@example.invalid commit --allow-empty -qm rejected
bad="$(git -C "$tmp" rev-parse HEAD)"
if bash "$tmp/scripts/check-author-policy.sh" "$good..$bad" > "$tmp/introduced.log" 2>&1; then
  echo "new unauthorized identity was accepted" >&2; exit 1
fi
grep -q "AUTHOR POLICY: FAILED" "$tmp/introduced.log"
echo "author range regressions passed: valid push accepted; new violation rejected"
