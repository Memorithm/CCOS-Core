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

# GitHub's exact squash/merge service identity is allowed as committer only.
GIT_AUTHOR_NAME="MEMOPERF" GIT_AUTHOR_EMAIL="contact@checkupauto.fr" \
  GIT_COMMITTER_NAME="GitHub" GIT_COMMITTER_EMAIL="noreply@github.com" \
  git -C "$tmp" commit --allow-empty -qm github-squash
github_good="$(git -C "$tmp" rev-parse HEAD)"
bash "$tmp/scripts/check-author-policy.sh" "$good..$github_good"

# The same service identity must never become an allowed author.
git -C "$tmp" -c user.name="GitHub" -c user.email=noreply@github.com commit --allow-empty -qm rejected-github-author
github_bad="$(git -C "$tmp" rev-parse HEAD)"
if bash "$tmp/scripts/check-author-policy.sh" "$github_good..$github_bad" > "$tmp/github-author.log" 2>&1; then
  echo "GitHub service identity was incorrectly accepted as an author" >&2; exit 1
fi
grep -q "author identity is outside" "$tmp/github-author.log"
git -C "$tmp" reset --hard -q "$github_good"
good="$github_good"

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
