#!/usr/bin/env bash
#
# Walk the whole chain for a published (or draft) release and print a transcript.
#
#   scripts/verify-release.sh v1.8.6 [<private-repo>]
#
# Every release so far was verified by typing the commands out and pasting the
# results into the QA record. That works, and it has two faults: what was
# actually run is only as accurate as the transcription, and a check nobody
# remembered to type is indistinguishable in the record from one that passed.
# So the commands live here, each one prints PASS or FAIL, and the exit status
# is the verdict.
#
# What it establishes, in the order a stranger would ask:
#
#   1. the files are what the list says, and the   shasum -a 256 -c,
#      list is every file there is                 and the set compared
#   2. the list is what the publisher vouches for  minisign
#   3. this workflow at this commit built them     gh attestation verify,
#                                                  workflow, tag and commit named
#   4. the .dmg is signed, notarized and stapled   verify-notarization.sh
#   5. the provenance record names this tag        PROVENANCE.txt
#   6. the public tree is what the private one emits  publisher re-run + diff
#
# Step 6 needs the private repository and is skipped with a stated reason when
# it is not to hand — it is the half a stranger cannot do, and the half that
# turns "the record says so" into "the record is true".
#
# Nothing here is fatal on its own until the end: the point is a transcript, so
# every check runs and the failures are collected rather than the first one
# stopping the rest.
set -uo pipefail

TAG="${1:?usage: verify-release.sh <tag> [private-repo-path]}"
PRIVATE="${2:-}"
VERSION="${TAG#v}"
REPO="jacobla1/sovatela"
# Verify files already downloaded instead of downloading them: the directory
# is copied, not used in place, so nothing in it is changed.
LOCAL="${SOVATELA_RELEASE_DIR:-}"
[ -n "$LOCAL" ] && LOCAL="$(cd "$LOCAL" && pwd)"

# What every release built in the public repository carries. Checked both
# ways: each of these must be in the signed list, and nothing outside the list
# may be on the release except the two documents attached beside it, which the
# signature does not cover and which are not programs.
INSTALLERS=(
  "Sovatela_${VERSION}_universal.dmg"
  "Sovatela_${VERSION}_x64-setup.exe"
  "Sovatela_${VERSION}_x64_en-US.msi"
  "Sovatela_${VERSION}_amd64.AppImage"
  "Sovatela_${VERSION}_amd64.deb"
  "Sovatela-${VERSION}-1.x86_64.rpm"
)
UNSIGNED_DOCUMENTS=("QA-${VERSION}.md" "TERMS-${VERSION}.md")

# Resolved before anything changes directory. Written as `$(dirname "$0")` at
# each use, it was evaluated from inside the download directory and every
# lookup missed — which the script reported as "not found" and counted as a
# skip, so the first run looked like two checks that could not be made rather
# than two that were broken. A skip that is really a bug is the worst line a
# transcript can carry.
HERE="$(cd "$(dirname "$0")" && pwd)"
[ -n "$PRIVATE" ] && PRIVATE="$(cd "$PRIVATE" && pwd)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

pass=0
fail=0
skip=0
ok()   { printf '  PASS  %s\n' "$1"; pass=$((pass + 1)); }
no()   { printf '  FAIL  %s\n' "$1"; fail=$((fail + 1)); }
meh()  { printf '  SKIP  %s\n' "$1"; skip=$((skip + 1)); }

echo "Sovatela release verification — $TAG"
echo "$(date -u '+%Y-%m-%dT%H:%M:%SZ') · $REPO"
echo

# ---------------------------------------------------------------------------
# The assets in a folder of their own: the checks below write their output to
# "$work", and step 1 now refuses any file here that the list does not name.
mkdir -p "$work/assets" && cd "$work/assets" || exit 1
if [ -n "$LOCAL" ]; then
  echo "Copying the assets from $LOCAL"
  cp -p "$LOCAL"/* . 2>/dev/null
else
  echo "Downloading the assets as published"
  if ! gh release download "$TAG" --repo "$REPO" >/dev/null 2>&1; then
    echo "  could not download $TAG from $REPO — is it a draft you cannot see?"
    exit 2
  fi
fi
ls -1 | sed 's/^/  /'
echo

# ---------------------------------------------------------------------------
echo "1. The files are what the list says, and the list is every file there is"
if [ -f SHA256SUMS.txt ]; then
  # Without --ignore-missing. With it, a file the list names could be absent
  # and the check still passed, and the line below counted the list's lines
  # rather than the files checked, so a list that covered nothing here read
  # as covering seven (1.10.2 launch review, R5).
  listed=$(awk 'NF >= 2 {sub(/^\*/, "", $2); print $2}' SHA256SUMS.txt | sort)
  count=$(printf '%s\n' "$listed" | grep -c .)
  if shasum -a 256 -c SHA256SUMS.txt >"$work/sums.out" 2>&1; then
    matched=$(grep -c ': OK$' "$work/sums.out")
    [ "$matched" -eq "$count" ] \
      && ok "every file the list names is here and matches ($matched of $count)" \
      || no "only $matched of the $count files the list names were checked"
  else
    no "shasum -a 256 -c"; sed 's/^/        /' "$work/sums.out"
  fi
  # The list names what a release carries.
  for want in "${INSTALLERS[@]}" PROVENANCE.txt; do
    printf '%s\n' "$listed" | grep -qxF "$want" \
      || no "the list does not name $want"
  done
  # And nothing is on the release that the list does not vouch for. An
  # installer beside the list but outside it is the shape a replaced or added
  # binary takes; this used to pass.
  stray=0
  for f in *; do
    [ -f "$f" ] || continue
    case "$f" in SHA256SUMS.txt|SHA256SUMS.txt.minisig) continue ;; esac
    printf '%s\n' "${UNSIGNED_DOCUMENTS[@]}" | grep -qxF "$f" && continue
    if ! printf '%s\n' "$listed" | grep -qxF "$f"; then
      no "$f is on the release but not in the signed list"
      stray=$((stray + 1))
    fi
  done
  [ "$stray" -eq 0 ] && ok "nothing on the release is outside the signed list"
else
  no "SHA256SUMS.txt is missing, so nothing below it can be checked"
fi
echo

# ---------------------------------------------------------------------------
echo "2. The list is what the publisher vouches for"
if [ ! -f SHA256SUMS.txt.minisig ]; then
  no "SHA256SUMS.txt.minisig is missing"
elif ! command -v minisign >/dev/null 2>&1; then
  meh "minisign is not installed here"
else
  # The key comes from the repository, never from the download directory: a
  # minisign.pub lying beside the files it is meant to vouch for is exactly
  # the key whoever replaced those files would bring. Releases up to 1.10.2
  # were signed with the key retired when signing moved behind the release
  # approval; it is kept in the repository so those signatures still verify.
  keyfile="minisign.pub"
  if [ "$(printf '%s\n%s\n' "$VERSION" "1.10.2" | sort -V | head -1)" = "$VERSION" ]; then
    keyfile="minisign-1.7.0-to-1.10.2.pub"
  fi
  pub=""
  for candidate in "$HERE/../$keyfile" ${PRIVATE:+"$PRIVATE/$keyfile"}; do
    [ -f "$candidate" ] && { pub="$candidate"; break; }
  done
  if [ -z "$pub" ]; then
    meh "$keyfile not found in the repository — the signature cannot be checked against a key"
  elif minisign -Vm SHA256SUMS.txt -p "$pub" >"$work/sig.out" 2>&1; then
    ok "minisign -Vm  ($(grep -i 'trusted comment' "$work/sig.out" | head -1 | sed 's/.*: //'))"
  else
    no "minisign -Vm"; sed 's/^/        /' "$work/sig.out"
  fi
fi
echo

# ---------------------------------------------------------------------------
echo "3. The release workflow, at this tag and commit, produced these files"
# With the workflow, the tag and the commit named. `--repo` alone accepts an
# attestation from any workflow in the repository, built from any branch or
# tag, which is not what this heading says (1.10.2 launch review, R5).
tagged=$(git ls-remote "https://github.com/$REPO" "refs/tags/$TAG^{}" 2>/dev/null | awk '{print $1}')
[ -n "$tagged" ] || tagged=$(git ls-remote "https://github.com/$REPO" "refs/tags/$TAG" 2>/dev/null | awk '{print $1}')
attested=0
for f in "${INSTALLERS[@]}" SHA256SUMS.txt; do
  if [ ! -f "$f" ]; then
    no "$f is not here to check its attestation"
    continue
  fi
  if gh attestation verify "$f" --repo "$REPO" \
      --signer-workflow "$REPO/.github/workflows/release.yml" \
      --source-ref "refs/tags/$TAG" \
      ${tagged:+--source-digest "$tagged"} >"$work/att.out" 2>&1; then
    attested=$((attested + 1))
  else
    no "gh attestation verify $f"; tail -3 "$work/att.out" | sed 's/^/        /'
  fi
done
want=$(( ${#INSTALLERS[@]} + 1 ))
[ "$attested" -eq "$want" ] \
  && ok "release.yml at $TAG (${tagged:0:12}) attested all $attested files" \
  || no "$attested of $want files carried an attestation from release.yml at $TAG"
echo

# ---------------------------------------------------------------------------
echo "4. The macOS build is signed, notarized and stapled"
dmg="Sovatela_${VERSION}_universal.dmg"
if [ ! -f "$dmg" ]; then
  meh "$dmg is not in the release — the macOS job may not have run"
elif [ ! -x "$HERE/verify-notarization.sh" ]; then
  meh "verify-notarization.sh not found"
else
  if "$HERE/verify-notarization.sh" "$PWD/$dmg" "$VERSION" >"$work/not.out" 2>&1; then
    ok "signed, notarized and stapled"
  else
    no "verify-notarization.sh"; tail -6 "$work/not.out" | sed 's/^/        /'
  fi
fi
echo

# ---------------------------------------------------------------------------
echo "5. The provenance record names this tag"
if [ ! -f PROVENANCE.txt ]; then
  no "PROVENANCE.txt is missing (releases before 1.8.4 carry none — for those this failure is the record's absence, not its loss)"
else
  grep -q "^tag: $TAG$" PROVENANCE.txt \
    && ok "names $TAG" || no "does not name $TAG"
  grep -q '"version": "'"$VERSION"'"' PROVENANCE.txt \
    && ok "quotes a source record for $VERSION" \
    || no "the quoted source record is not for $VERSION"
  commit=$(grep '^public_commit: ' PROVENANCE.txt | awk '{print $2}')
  tagged=$(git ls-remote "https://github.com/$REPO" "refs/tags/$TAG^{}" 2>/dev/null | awk '{print $1}')
  if [ -n "$commit" ] && [ "$commit" = "$tagged" ]; then
    ok "public_commit is the commit $TAG points at (${commit:0:12})"
  else
    no "public_commit $commit is not what $TAG points at ($tagged)"
  fi
  grep -q "PROVENANCE.txt" SHA256SUMS.txt 2>/dev/null \
    && ok "is itself covered by the signed checksum list" \
    || no "is not in SHA256SUMS.txt, so the signature does not cover it"
fi
echo

# ---------------------------------------------------------------------------
echo "6. The public tree is what the private one emits"
if [ -z "$PRIVATE" ]; then
  meh "no private repository given — pass its path as the second argument"
elif [ ! -f "$PRIVATE/deploy/publish-source.mjs" ]; then
  meh "$PRIVATE does not look like the private repository"
else
  record_commit=$(sed -n 's/.*"private_commit": "\([0-9a-f]*\)".*/\1/p' PROVENANCE.txt | head -1)
  if [ -z "$record_commit" ]; then
    no "the provenance record names no private commit"
  else
    # Published from a worktree at the recorded commit, rather than from the
    # repository as it stands.
    #
    # Work continues after a tag is cut — this script was itself written after
    # v1.8.4 — so by the time anyone verifies, `main` has moved on. Requiring a
    # detached checkout of the release commit would make verification disruptive
    # enough to be skipped, and the publisher refuses a dirty tree, so a
    # half-finished change in the working copy would block it entirely. A
    # worktree leaves the repository exactly as it was found.
    if ! git -C "$PRIVATE" cat-file -e "${record_commit}^{commit}" 2>/dev/null; then
      no "the private repository does not contain ${record_commit:0:12}"
    else
      rm -rf "$work/private" "$work/republished" "$work/mirror"
      if ! git -C "$PRIVATE" worktree add --detach -q "$work/private" "$record_commit" 2>"$work/wt.out"; then
        meh "could not make a worktree at ${record_commit:0:12}"
        sed 's/^/        /' "$work/wt.out"
      else
        if (cd "$work/private" && node deploy/publish-source.mjs "$work/republished" >"$work/pub.out" 2>&1); then
          if git clone -q --depth 1 --branch "$TAG" "https://github.com/$REPO" "$work/mirror" 2>/dev/null; then
            rm -rf "$work/mirror/.git"
            if diff -r "$work/republished" "$work/mirror" >"$work/diff.out" 2>&1; then
              ok "re-running the publisher at ${record_commit:0:12} reproduces the published tree exactly"
            else
              no "the published tree differs from what the publisher emits"
              head -20 "$work/diff.out" | sed 's/^/        /'
            fi
          else
            meh "could not clone $TAG to compare against"
          fi
        else
          no "the publisher would not run at ${record_commit:0:12}"
          tail -5 "$work/pub.out" | sed 's/^/        /'
        fi
        git -C "$PRIVATE" worktree remove --force "$work/private" 2>/dev/null
      fi
    fi
  fi
fi
echo

# ---------------------------------------------------------------------------
echo "──────────────────────────────────────────"
printf 'passed %d · failed %d · skipped %d\n' "$pass" "$fail" "$skip"
if [ "$fail" -gt 0 ]; then
  echo "This release does not verify. Nothing above is a formality —"
  echo "each failure is a claim the release makes that is not true."
  exit 1
fi
if [ "$skip" -gt 0 ]; then
  echo "Everything checked passed. The skipped lines are checks that were not"
  echo "made, and a transcript that hides them is worth less than one that does not."
fi
exit 0
