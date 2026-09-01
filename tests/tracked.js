import { execFileSync } from "node:child_process";

// The set of files git tracks, or null when that cannot be determined.
//
// Suites that walk a directory with readdirSync see whatever is on the disk,
// including files git ignores. That made the suite's own test count depend on
// the machine it ran on: ten ignored working documents in docs/ produced twenty
// extra tests here that no clean checkout, no CI run and no reviewer could
// reproduce — and the figure was quoted as release evidence before anyone
// noticed. A local scratch file could also have failed a check about what the
// project ships, which it does not.
//
// Returns null rather than throwing when there is no git — a source tarball is
// a legitimate way to run these — and callers walk the disk unfiltered in that
// case, which is the old behaviour and the best available answer there.
export function trackedFiles(repo) {
  try {
    return new Set(
      execFileSync("git", ["ls-files", "-z"], {
        cwd: repo,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "ignore"],
      })
        .split("\0")
        .filter(Boolean),
    );
  } catch {
    return null;
  }
}

// Keep only what git tracks, given absolute paths under `repo`.
//
// `git ls-files` always reports forward slashes; `path.join` on Windows
// produces backslashes. Comparing the two directly matched almost nothing on
// the windows-latest runner, which filtered the file list down to two entries
// and failed the suite that checks there are files to check at all.
export function onlyTracked(repo, absolutePaths) {
  const tracked = trackedFiles(repo);
  if (!tracked) return absolutePaths;
  const relative = (f) => f.slice(repo.length + 1).split("\\").join("/");
  return absolutePaths.filter((f) => tracked.has(relative(f)));
}
