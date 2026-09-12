import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { trackedFiles } from "./tracked.js";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// The release workflow runs with the Apple signing secrets and a token that can
// write releases. A tag is a mutable reference — it can be moved to point at
// different code — so an action referenced by tag is code that can change
// underneath a pipeline holding those secrets. A full commit SHA is the only
// immutable reference GitHub offers.
describe("third-party Actions are pinned to commit SHAs", () => {
  const workflows = readdirSync(join(repo, ".github/workflows")).filter((f) =>
    f.endsWith(".yml"),
  );

  it("finds workflows to check", () => {
    expect(workflows.length).toBeGreaterThan(0);
  });

  for (const file of workflows) {
    it(file, () => {
      const src = read(join(".github/workflows", file));
      // `- uses:` (list item) and `uses:` (mapping key) are both ordinary YAML
      // and both appear in these workflows. An earlier version of this pattern
      // required the mapping form, so every `- uses:` step went unexamined and
      // the test passed over an unpinned action.
      const refs = [...src.matchAll(/^\s*(?:-\s*)?uses:\s*(\S+)/gm)].map(
        (m) => m[1],
      );

      // A pattern that silently matches nothing passes a test like this, so
      // the count has to be accounted for rather than assumed.
      const occurrences = (src.match(/\buses:/g) ?? []).length;
      expect(refs.length, "some `uses:` lines were not examined").toBe(
        occurrences,
      );

      const unpinned = refs.filter((ref) => {
        // A local reusable workflow is this repository's own code.
        if (ref.startsWith("./")) return false;
        return !/^[0-9a-f]{40}$/.test(ref.split("@")[1] ?? "");
      });
      expect(
        unpinned,
        `pinned to a movable reference:\n  ${unpinned.join("\n  ")}`,
      ).toEqual([]);
    });
  }

  it("Dependabot keeps the pins from going stale", () => {
    // A pin nobody updates is its own problem.
    const cfg = read(".github/dependabot.yml");
    expect(cfg).toContain("github-actions");
  });
});

// package-lock.json sat at 1.5.0 through four releases: npm only rewrites the
// version there when it runs, and bumping package.json by hand does not.
// Nothing used the stale number, but it is published, and it disagreed with
// every other file that states a version.
describe("every file that states the version agrees", () => {
  const pkg = JSON.parse(read("package.json"));

  it("package-lock.json", () => {
    const lock = JSON.parse(read("package-lock.json"));
    expect(lock.version).toBe(pkg.version);
    expect(lock.packages?.[""]?.version).toBe(pkg.version);
  });

  it("tauri.conf.json", () => {
    expect(JSON.parse(read("src-tauri/tauri.conf.json")).version).toBe(pkg.version);
  });

  it("Cargo.toml", () => {
    const toml = read("src-tauri/Cargo.toml");
    expect(toml.match(/^version = "([^"]+)"/m)?.[1]).toBe(pkg.version);
  });

  it("Cargo.lock", () => {
    const lock = read("src-tauri/Cargo.lock");
    const at = lock.indexOf('name = "scale"');
    expect(lock.slice(at, at + 120).match(/version = "([^"]+)"/)?.[1]).toBe(pkg.version);
  });

  it("the changelog's newest entry", () => {
    const first = read("CHANGELOG.md").match(/^## (\d+\.\d+\.\d+)/m)?.[1];
    expect(first).toBe(pkg.version);
  });

  it("the release notes", () => {
    const title = read("docs/release/RELEASE-NOTES.md").match(/^# Release notes — Sovatela (\S+)/m)?.[1];
    expect(title).toBe(pkg.version);
  });

  // Conditional, because `deploy/publish-source.mjs` withholds this file from
  // the public mirror on purpose and repoints links at sovatela.eu/accessibility.
  // Requiring it unconditionally made the *published* source fail its own suite
  // — which is how an external audit found it — while passing in the working
  // repo, where the file exists. A withheld document is not a missing one, and
  // the fix is not to write a replacement into the mirror: that would publish
  // something deliberately withheld, in a lossier form than the original.
  it("the accessibility statement, where it is present", () => {
    const path = join(repo, "docs/ACCESSIBILITY.md");
    if (!existsSync(path)) return; // the public mirror: withheld by design
    const applies = read("docs/ACCESSIBILITY.md").match(/Applies to: Sovatela v(\S+)/)?.[1];
    expect(applies).toBe(pkg.version);
  });

  // TERMS.md carried a version stamp, and it went stale twice: it read
  // "Applies to: Sovatela v1.2.0" while 1.6.0 was public, and then
  // "v1.6.1" while the v1.6.1 tag contained the unpublished draft. The stamp
  // is written before the tag is cut, so it can only ever describe a release
  // it has not seen — and a test asserting it matched package.json enforced
  // the false claim rather than catching it.
  //
  // An effective date is the honest form: it speaks forwards only. So this
  // guard now does the opposite of what it used to — it keeps the version
  // stamp out.
  it("the terms of use carry an effective date, not a version", () => {
    const terms = read("docs/TERMS.md");
    expect(terms, "TERMS.md has no effective date").toMatch(
      /^Effective from \d{4}-\d{2}-\d{2}\b/m,
    );
    expect(
      terms,
      "TERMS.md claims to apply to a version again — the tag for that version " +
        "cannot contain a page written after it was cut",
    ).not.toMatch(/Applies to: Sovatela v/);
  });

  // This used to assert that TERMS.md carried a review due-date and an owner:
  // the finding no code closes, and therefore the one that rots. It was closed
  // on 2026-09-01 by a decision rather than by a review — the sections that
  // needed a lawyer were the ones limiting liability, and they were removed
  // instead of published unreviewed. So the guard changes shape. It no longer
  // protects a pending review; it protects the decision from being quietly
  // reversed.
  describe("the terms are published, and stay publishable", () => {
    const terms = read("docs/TERMS.md");
    const build = read("deploy/web/build.mjs");
    const publicPart = terms.slice(0, terms.indexOf("<!-- public:end -->"));

    it("no longer hangs a pending legal review over a shipped product", () => {
      // Scoped to what is published. The maintainer tail quotes the banner it
      // is explaining the removal of, and that is the point of the tail.
      for (const banner of [/Review due by/i, /awaiting .{0,20}legal review/i,
                            /^#\s.*\boutline\b/im, /Not yet published/i]) {
        expect(publicPart, `TERMS.md carries a pending-review banner again: ${banner}`)
          .not.toMatch(banner);
      }
    });

    it("is not held back from the site", () => {
      const entry = build.match(/\["terms", "docs\/TERMS\.md"[^\]]*\]/s)?.[0];
      expect(entry, "the terms are no longer in PAGES").toBeTruthy();
      expect(entry, "the terms are held again — if that is deliberate, say why here")
        .not.toContain("hold:");
    });

    it("does not reintroduce an unreviewed liability exclusion", () => {
      // The removed sections are the ones a consumer is least likely to be
      // bound by and a lawyer most needed to see. Putting them back is a
      // decision that has to be made deliberately, not by pasting the old text.
      for (const clause of [/^#+.*Limitation of liability/im,
                            /to the fullest extent the law allows/i,
                            /we are not liable for indirect/i]) {
        expect(publicPart, `an exclusion clause is back in TERMS.md: ${clause}`)
          .not.toMatch(clause);
      }

      // Matching the old draft's wording only catches the old draft. §4 kept
      // allocating the cost of a defect to the user for a full revision after
      // the exclusions were removed, because it was phrased as a statement of
      // fact — "charges you did not expect, whether from ... a defect in this
      // software" — and nothing here was looking for the indicative mood.
      //
      // The tell is not the verb, it is a named cause of loss sitting inside a
      // sentence that says who bears it. So look for the causes.
      for (const disguised of [/defects? in this software/i,
                               /caused by (?:a )?(?:defect|bug|fault)/i,
                               /(?:are|is) between you and (?:that|your) provider/i]) {
        expect(
          publicPart,
          "TERMS.md allocates the cost of a defect to the user again, as a " +
            `statement of fact rather than as an exclusion: ${disguised}`,
        ).not.toMatch(disguised);
      }
    });

    // §2 promises the page you accepted can be read rather than reconstructed.
    // Nothing in the repository made that true — so it was a claim of the same
    // kind as the version stamp it replaced, which is the defect, not the fix.
    // The release workflow attaches the page at the moment the release is cut.
    it("is archived beside the release, as §2 says it is", () => {
      const publicTerms = publicPart;
      expect(publicTerms, "§2 no longer promises an archived copy").toMatch(
        /archives the page as it stood/i,
      );

      const release = read(".github/workflows/release.yml");
      expect(
        release,
        "TERMS.md is no longer attached to the release, so §2 promises a copy " +
          "that will not exist",
      ).toMatch(/gh release upload "\$TAG" "\$out"/);
      expect(release, "the archived name does not carry the version").toMatch(
        /out="TERMS-\$\{version\}\.md"/,
      );
      // The maintainer's tail is not a term. Archiving it would publish an
      // internal record as though people had agreed to it.
      expect(release, "the archive no longer stops at the public marker").toContain(
        "public:end",
      );
    });

    it("keeps the record of what was dropped and why", () => {
      expect(terms).toContain("Why this document is short");
      expect(terms).toMatch(/remains open/i);
    });

    it("sends complaints to the publisher rather than onward", () => {
      expect(publicPart).toMatch(/\*\*Complaints come here:\*\*/);
      expect(publicPart).toContain("info@anaubi.com");
    });
  });

  // Missed in the 1.6.2 bump and caught by deploy/web/build.mjs, which refuses
  // to publish a page whose stamp disagrees with the release. That guard works,
  // but it only fires at release time — by which point the tag is cut and the
  // installers are built. This is the same check, on the commit that causes it.
  it("the privacy policy", () => {
    const applies = read("docs/PRIVACY.md").match(/Applies to: Sovatela v(\S+)/)?.[1];
    expect(applies).toBe(pkg.version);
  });

  it("the technical specification", () => {
    const v = read("docs/TECHNICAL-SPEC.md").match(/^Sovatela v(\S+)/m)?.[1];
    expect(v).toBe(pkg.version);
  });
});

// The release build moved to the public repository so that build attestations
// could exist: GitHub publishes them free for public repos and charges
// Enterprise Cloud for private ones, so a private build could be signed by
// Apple and still not prove which commit produced it. That put the Apple
// certificate in a public repository's secrets, which is safe only while
// specific properties hold. These are those properties.
describe("the release builds where its provenance can be published", () => {
  const release = read(".github/workflows/release.yml");
  const workflows = readdirSync(join(repo, ".github/workflows")).filter((f) =>
    f.endsWith(".yml"),
  );

  it("runs in the public repository", () => {
    expect(release).toMatch(/github\.repository == 'jacobla1\/sovatela'/);
    expect(
      release,
      "the release still gates on the private repository, so it will never run",
    ).not.toMatch(/github\.repository == 'jacobla1\/Scale'/);
  });

  // Notarization used to be verified by the maintainer running a script by hand
  // between the build and the publish. That is a step, and steps get skipped on
  // the release someone is in a hurry for — while the failure it exists to
  // catch, an unsigned build from a lapsed certificate, is exactly the one that
  // looks fine in the log.
  it("verifies notarization on the artifact, in the workflow", () => {
    expect(release, "nothing in the release runs the notarization check").toMatch(
      /verify-notarization\.sh/,
    );

    const jobs = release.split(/\n  (?=[a-z][a-z0-9-]*:\n)/);
    const verifier = jobs.find((j) => /verify-notarization\.sh/.test(j));
    expect(verifier, "the notarization check is not inside a job").toBeTruthy();
    expect(
      verifier,
      "the notarization check does not run on macOS, where spctl exists",
    ).toMatch(/runs-on:\s*macos/);

    // It also has to gate something, or it is a job whose failure nobody waits
    // for before publishing.
    const assets = jobs.find((j) => /^\s*verify-release-assets:/m.test(j));
    expect(assets, "verify-release-assets is gone").toBeTruthy();
    expect(
      assets,
      "the checksums and attestations no longer wait for the signature check",
    ).toMatch(/needs:.*verify-macos-signature/);
  });

  // The one that matters most. A secret is only as scoped as the number of
  // jobs that can read it.
  it("lets exactly one job see the Apple certificate", () => {
    const jobs = release.split(/\n  (?=[a-z][a-z0-9-]*:\n)/);
    const withApple = jobs.filter((j) => /APPLE_CERTIFICATE:/.test(j));
    expect(
      withApple.length,
      "more than one job can read the signing certificate",
    ).toBe(1);
    expect(withApple[0], "the signing job is not named for what it does").toMatch(
      /^\s*build-macos:/m,
    );
    expect(
      withApple[0],
      "the signing job no longer sits behind an approval environment",
    ).toMatch(/environment: release/);
  });

  // These are the triggers that hand secrets to code from a fork. Neither has
  // ever been used here; in a public repository that has to stay true.
  it("uses no trigger that runs untrusted code with secrets", () => {
    for (const f of workflows) {
      const src = read(join(".github/workflows", f));
      expect(src, `${f} uses pull_request_target`).not.toMatch(
        /^\s*pull_request_target:/m,
      );
      expect(src, `${f} uses workflow_run`).not.toMatch(/^\s*workflow_run:/m);
    }
  });

  it("only builds from a tag", () => {
    const on = release.slice(release.indexOf("on:"), release.indexOf("jobs:"));
    expect(on).toMatch(/tags:/);
    expect(on, "the release can be triggered by something other than a tag").not.toMatch(
      /workflow_dispatch|pull_request|schedule/,
    );
  });

  it("publishes checksums, a signature over them, and provenance", () => {
    expect(release, "checksums are no longer generated in CI").toMatch(
      /shasum -a 256/,
    );
    expect(release, "the checksums are no longer signed").toMatch(/minisign -S/);
    expect(release, "there is no build attestation").toMatch(
      /attest-build-provenance/,
    );
    // A signing step that degrades quietly when its key is missing produces
    // releases that claim to be signed and are not — the exact defect the Apple
    // secrets already taught this project.
    expect(release, "a missing signing key no longer fails the release").toMatch(
      /MINISIGN_SECRET_KEY is not set/,
    );
  });

  // The Windows terminal-access gate is called by the release. Gated to the
  // private repo alone it would *skip* rather than fail, and a skipped job
  // satisfies `needs:` — so the gate would silently stop being one.
  it("keeps the Windows terminal-access gate reachable from the release", () => {
    const win = read(".github/workflows/windows-terminal-access.yml");
    expect(win).toMatch(/jacobla1\/sovatela/);
  });
});

// The conformance sentence has now been wrong twice, in the same direction.
//
// First "partially conformant with WCAG 2.1 level AA" — a formulation reserved
// for content outside the author's control, not for the author's own known
// gaps. Corrected to "does not currently conform fully", which a second review
// caught as the same claim in quieter clothes: "not fully" is read as "mostly",
// which is the impression the rule exists to prevent, on a page that spends a
// paragraph explaining why.
//
// Twice is a pattern, and a pattern gets a test. This forbids the qualifiers
// rather than prescribing a sentence, because the next wrong version will be
// worded differently and the failure is always the hedge.
describe("the accessibility statement does not hedge its conformance claim", () => {
  // Withheld from the public mirror by design, like the version-stamp check
  // above: a withheld document is not a missing one.
  const path = join(repo, "docs/ACCESSIBILITY.md");

  it("states it plainly, or is absent from this tree", () => {
    if (!existsSync(path)) return; // the public mirror: withheld by design
    const text = read("docs/ACCESSIBILITY.md").replace(/\s+/g, " ");
    const status = text.match(/Conformance status: ([^*]+)/)?.[1] ?? "";
    expect(status, "no conformance status to check").not.toBe("");
    expect(status, "the conformance claim is hedged").not.toMatch(
      /partial|fully|mostly|largely|substantially|currently/i,
    );
    expect(status).toMatch(/does not conform to WCAG/i);
  });
});

// Three documents disagreed about Windows signing: SECURITY.md called it a
// decision and permanent, the README listed it on the roadmap, and the website
// said "not signed yet". A reader deciding whether to trust the installer got a
// different answer depending on which page they opened.
describe("Windows signing is described the same way everywhere", () => {
  // **Whitespace is normalised before matching, and that is the whole point of
  // this line.** This guard was written for the literal string "not signed yet"
  // on the website, and it did not catch it: the sentence wrapped between
  // "signed" and "yet", so the file held "not signed\n      yet" and a regex
  // with a single space in it matched nothing. The claim it was written to
  // forbid shipped on the live download page through 1.8.5 and was found by an
  // external review, not by this test.
  //
  // A guard that reads prose out of a source file has to read it the way a
  // person does. Hard-wrapping is not a semantic act, and any check that treats
  // a newline as different from a space is one reflow away from silence.
  //
  // **The same lesson, a second time.** 1.8.8 widened this to survive a line
  // break and an HTML tag, and an external review of 1.8.8 then walked through
  // it with `not signed&nbsp;yet`: an entity is not whitespace until something
  // decodes it, and nothing here did. Twice now this guard has been taught the
  // instance it was shown. So the rule is stated once, as the class: read the
  // document the way a browser and a person would, then compare.
  //
  // That means entities become their characters, characters that are invisible
  // to a reader stop separating words, and every kind of Unicode space — not
  // just ASCII — collapses. A word split by a zero-width space or a soft hyphen
  // reads as one word on the page and must match as one word here.
  const named = {
    nbsp: " ", amp: "&", lt: "<", gt: ">", quot: '"', apos: "'",
    ndash: "–", mdash: "—", hellip: "…", shy: "­",
    thinsp: " ", ensp: " ", emsp: " ",
    zwj: "‍", zwnj: "‌", nbhy: "‑",
  };
  const flatten = (s) =>
    s
      .replace(/<[^>]*>/g, " ")
      .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
      .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(Number(d)))
      .replace(/&([a-z]+);/gi, (m, n) => named[n.toLowerCase()] ?? m)
      .replace(/[*`]/g, "")
      // Invisible to a reader, so invisible to the match.
      .replace(/[­​‌‍⁠﻿]/g, "")
      // Every Unicode space, not only the ASCII ones.
      .replace(/[\s   -   　]+/g, " ");
  const paths = trackedFiles(repo) ?? new Set(readdirSync(repo, { recursive: true })
    .filter((p) => !/(^|[/\\])(?:node_modules|target|dist|\.git)([/\\]|$)/.test(p)));
  const docs = Object.fromEntries([...paths]
    .filter((p) => /\.(?:md|markdown|html?)$/i.test(p))
    .map((p) => [p, flatten(read(p))]));
  const forbidden = /not signed yet|signing,? so SmartScreen stops warning|signing is planned|signing (?:isn't|isn’t|is not) configured yet/gi;

  // Exact historical quotations of corrected claims, not excluded files.
  // A new claim elsewhere in one of these records must still fail the guard.
  const historical = {
    "CHANGELOG.md": ['The download page said Windows and Linux are "not signed yet".'],
    "docs/release/RELEASE-NOTES.md": ['The download page said Windows and Linux builds are "not signed yet".'],
    "docs/release/REVIEW-1.8.5.md": [
      'The guard against "not signed yet" could not fire',
      '"verify every claim", "not signed yet", the update check',
    ],
    "docs/release/QA-1.8.6.md": [
      'The test forbidding "not signed yet" in any document did not catch',
      '| Windows/Linux "not signed yet" | unsigned by choice; signing is not planned |',
      'the phrase not signed yet returns nothing on the live page',
    ],
  };

  it("is recorded as a decision, not as pending work", () => {
    expect(docs["SECURITY.md"]).toMatch(/Windows signing is not planned/i);
  });

  it("includes FAQ and troubleshooting in the tracked-document sweep", () => {
    expect(docs["docs/FAQ.md"]).toBeTruthy();
    expect(docs["docs/TROUBLESHOOTING.md"]).toBeTruthy();
  });

  it("recognises wrapped and formatted promises of future signing", () => {
    for (const example of [
      "Windows builds are not signed\n yet",
      "Windows code signing isn't configured\n yet",
      "Windows signing is not <strong>configured</strong> yet",
    ]) expect(flatten(example)).toMatch(forbidden);
  });

  // Each of these reads as the forbidden sentence on a rendered page and slipped
  // past the guard as source. The first is the one an external review used
  // against 1.8.8; the rest are the same trick spelled differently, and they are
  // here because naming only the one that was demonstrated is how this guard
  // came to need widening twice.
  it("reads entities and invisible characters the way a page does", () => {
    for (const example of [
      "Windows builds are not signed&nbsp;yet",
      "Windows builds are not signed&#160;yet",
      "Windows builds are not signed&#xA0;yet",
      "Windows builds are not signed yet",
      "Windows builds are not sig­ned yet",
      "Windows builds are not signed yet",
      "Windows builds are not signed　yet",
      "Windows signing is not <b>configured</b>&nbsp;yet",
    ]) expect(flatten(example)).toMatch(forbidden);
  });

  // The other half. A guard that rewrites the document until everything matches
  // is no guard, so the decoding must not invent the phrase where it is absent.
  // A zero-width space is not a space. It renders as nothing, so "signed​yet"
  // reads as one word on the page and is not the forbidden claim; joining the
  // words is the right answer, not a miss.
  it("does not manufacture a match out of unrelated text", () => {
    for (const example of [
      "Windows builds are not signed​yet",
      "Windows builds are signed and notarized.",
      "not signed &amp; not planned",
      "signing is not planned, and that is a decision",
    ]) expect(flatten(example)).not.toMatch(/not signed yet/);
  });

  it("is not on the roadmap in any tracked document", () => {
    for (const [name, text] of Object.entries(docs)) {
      let current = text;
      for (const quote of historical[name] ?? []) {
        expect(current, `historical exception has gone stale: ${name}: ${quote}`).toContain(quote);
        current = current.replace(quote, "");
      }
      expect(current, `${name} implies Windows signing is coming`).not.toMatch(forbidden);
    }
  });
});

// A publish that refuses is the last line of defence, not the first. A stray
// directory left in the working tree by a mistyped command was swept in by
// `git add -A` — 242 duplicated files, which then blocked publication until
// somebody read the refusal and worked out where they had come from.
//
// Nothing shipped, because the classification gate is fail-closed. This is the
// earlier warning: a tracked path that cannot be a deliberate one.
describe("nothing that looks like a stray build artifact is tracked", () => {
  const tracked = [...(trackedFiles(repo) ?? [])];

  it("has files to check", () => {
    expect(tracked.length).toBeGreaterThan(100);
  });

  it("tracks no path whose directory looks like a command-line flag", () => {
    // `--dry-run/` is the case that happened: this script takes no options, so
    // the flag was read as the directory to write into.
    const flagged = tracked.filter((p) => p.split("/").some((seg) => seg.startsWith("-")));
    expect(
      flagged,
      "a mistyped command left a directory in the tree and it was committed",
    ).toEqual([]);
  });

  it("tracks no second copy of its own published tree", () => {
    // The shape of the accident rather than its name: any directory holding a
    // nested `src-tauri/tauri.conf.json` is a copy of this repository inside
    // itself.
    const nested = tracked.filter(
      (p) => p.endsWith("src-tauri/tauri.conf.json") && p !== "src-tauri/tauri.conf.json",
    );
    expect(nested, "the repository contains a copy of itself").toEqual([]);
  });
});

// Everything this project claims about correspondence — that the public tree is
// what the private one emits, that a release was built from the reviewed
// source — was checked by a person re-running the publisher and comparing
// directories. That is real evidence and nobody else can hold it: a reader of
// the public repository cannot ask which private commit produced it.
//
// The provenance record states it instead, and the release signs it.
describe("the published tree says where it came from", () => {
  const publisher = read("deploy/publish-source.mjs");
  const workflow = read(".github/workflows/release.yml");

  it("writes a record naming the private commit and a digest of what shipped", () => {
    expect(publisher).toMatch(/PROVENANCE\.json/);
    for (const field of ["private_commit", "payload_sha256", "version", "files"]) {
      expect(publisher, `the record no longer carries ${field}`).toContain(field);
    }
  });

  it("digests the published files themselves, not just their names", () => {
    // A digest over paths alone would match a tree whose contents had changed,
    // which is the one thing it exists to detect.
    const shared = read("deploy/payload-digest.mjs");
    const at = shared.indexOf("export function payloadDigest");
    expect(at, "the digest is gone").toBeGreaterThan(-1);
    const body = shared.slice(at, shared.indexOf("\n}", at));
    expect(body).toMatch(/readFileSync/);
  });

  it("digests the executable bit along with the bytes", () => {
    // A published script that arrives executable when the private one was not,
    // or the reverse, is a difference in what the tree *does* — and a digest
    // over path and content alone calls those two trees identical.
    const shared = read("deploy/payload-digest.mjs");
    const at = shared.indexOf("export function payloadDigest");
    const body = shared.slice(at, shared.indexOf("\n}", at));
    expect(body, "the digest ignores file modes").toMatch(/statSync|mode/);
  });

  it("does not call it a digest of the tree, which it is not", () => {
    // It covers the payload, and the published directory has one more file in
    // it — PROVENANCE.json cannot contain its own hash. The old name invited
    // exactly the reading the record cannot support.
    expect(publisher).toContain("payload_sha256");
    const at = publisher.indexOf("const provenance = {");
    const body = publisher.slice(at, publisher.indexOf("};", at));
    expect(body, "the field is called a tree digest again").not.toMatch(/^\s*tree_sha256:/m);
  });

  it("recomputes the digest over the tree it is about to sign", () => {
    // The workflow used to copy the record into a signed asset having checked
    // only its version and the presence of some fields. It could not prove the
    // private commit produced this tree — that needs the private repository —
    // but it could prove the record describes the files it is shipping, and it
    // was not doing that at all.
    const verify = workflow.search(/payload-digest\.mjs[^\n]*--verify/);
    expect(verify, "the workflow signs the record without recomputing it").toBeGreaterThan(-1);
    const signs = workflow.indexOf("shasum -a 256 PROVENANCE.txt >> SHA256SUMS.txt");
    expect(verify, "the digest is checked after the record is already signed").toBeLessThan(signs);
  });

  it("counts what git tracks, not what is lying in the directory", async () => {
    // This killed the first release that used the check. The workflow
    // downloads the installers into `release-assets/` *inside* the workspace
    // and then verifies that workspace, so a digest that walked the disk saw
    // twelve build artefacts as published source and reported that the record
    // did not describe a tree it described perfectly well.
    //
    // The failure mode is the one worth guarding: not a wrong digest, but a
    // correct record rejected — a gate that cries wolf gets removed, and then
    // it is not a gate.
    const { execFileSync } = await import("node:child_process");
    const { mkdtempSync, writeFileSync, mkdirSync } = await import("node:fs");
    const { tmpdir } = await import("node:os");
    const { payloadFiles } = await import("../deploy/payload-digest.mjs");

    const dir = mkdtempSync(join(tmpdir(), "payload-"));
    writeFileSync(join(dir, "README.md"), "# published\n");
    writeFileSync(join(dir, "PROVENANCE.json"), "{}\n");
    const git = (...args) => execFileSync("git", ["-C", dir, ...args], { stdio: "ignore" });
    git("init", "-q");
    git("config", "user.email", "t@example.invalid");
    git("config", "user.name", "t");
    git("add", "-A");
    git("commit", "-qm", "published");

    // Now the workspace as the release job leaves it.
    mkdirSync(join(dir, "release-assets"));
    writeFileSync(join(dir, "release-assets", "Sovatela_9.9.9_universal.dmg"), "x");
    writeFileSync(join(dir, "release-assets", "SHA256SUMS.txt"), "x");

    expect(
      payloadFiles(dir),
      "downloaded artefacts were counted as published source",
    ).toEqual(["README.md"]);
  });

  it("records a digest of what it staged, not of the target's git index", async () => {
    // 1.8.5 was published with a PROVENANCE.json that did not describe its own
    // tree, and the release job would have refused it at the last step — the
    // same place `v1.8.3` died, for a sibling reason.
    //
    // The publisher asked `payloadDigest(target)` to work out the file list for
    // itself. Given a directory that is the root of a repository, it answers
    // with what git *tracks* — and the mirror's index, at the moment the
    // publisher runs, is still the previous release's. So the digest covered
    // last release's list of paths hashed against this release's bytes, while
    // `files:` beside it was counted from the staged set. A record describing
    // two different trees at once.
    //
    // It passed in 1.8.4 purely by ordering: that publish followed a
    // `git add -A`, so the index already agreed. Nothing enforced that, and the
    // next release quietly broke it.
    //
    // The target here is a repository whose index is deliberately stale — one
    // committed file standing in for the previous release, one new file on disk
    // that the publisher has just staged and nobody has added yet.
    //
    // Driving the real publisher was tried and is the wrong instrument: it
    // refuses to run while the private working tree is dirty, so the test would
    // pass or fail on whether someone had unsaved work, and the failure it
    // produced was that refusal rather than anything about a digest. A test that
    // cannot be run while you are working on the thing it guards gets run once.
    const { execFileSync } = await import("node:child_process");
    const { mkdtempSync, writeFileSync, rmSync } = await import("node:fs");
    const { tmpdir } = await import("node:os");
    const { payloadDigest, verifyPayload } = await import("../deploy/payload-digest.mjs");

    const dir = mkdtempSync(join(tmpdir(), "stale-index-"));
    try {
      const git = (...args) => execFileSync("git", ["-C", dir, ...args], { stdio: "ignore" });
      git("init", "-q");
      git("config", "user.email", "t@example.invalid");
      git("config", "user.name", "t");
      writeFileSync(join(dir, "README.md"), "# published\n");
      git("add", "-A");
      git("commit", "-qm", "previous release");

      // This release adds a file. On disk, not yet in the index — exactly the
      // state the mirror is in when the publisher runs.
      writeFileSync(join(dir, "NEW.md"), "# new in this release\n");
      const staged = ["NEW.md", "README.md"];

      // The trap: same directory, two different answers. If these ever agree,
      // this test has stopped reproducing the defect and needs rewriting rather
      // than believing.
      const fromIndex = payloadDigest(dir);
      const fromStaged = payloadDigest(dir, staged);
      expect(fromIndex, "the stale index must give a different answer").not.toBe(fromStaged);

      // And the staged answer is the correct one, not merely a different one:
      // once the index catches up, it is what the release job recomputes.
      writeFileSync(
        join(dir, "PROVENANCE.json"),
        JSON.stringify({
          schema: 2,
          private_commit: "0".repeat(40),
          files: staged.length,
          payload_sha256: fromStaged,
        }),
      );
      git("add", "-A");
      expect(verifyPayload(dir)).toEqual([]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("passes the staged list to the digest rather than letting it guess", () => {
    // The behavioural test above proves the two answers differ. This one pins
    // which of them the publisher asks for, because that is a single argument
    // and it is the whole of the defect.
    expect(publisher).toMatch(/payloadDigest\(\s*target\s*,/);
  });

  it("keeps one copy of the digest construction, not two", () => {
    // The publisher writes the digest and the workflow now checks it. Written
    // out twice, the two drift the first time either is edited — and a record
    // that no longer means what it says is worse than no record, because it
    // still verifies.
    expect(publisher).toMatch(/from "\.\/payload-digest\.mjs"/);
    const shared = read("deploy/payload-digest.mjs");
    expect(shared).toMatch(/export function payloadDigest/);
    // The publisher must not have kept its own hashing loop beside the import.
    const at = publisher.indexOf("const provenance = {");
    const before = publisher.slice(Math.max(0, at - 800), at);
    expect(
      before,
      "the publisher still builds the digest itself as well as importing it",
    ).not.toMatch(/createHash\("sha256"\)[\s\S]*update\("\\0"\)/);
  });

  it("refuses to sign a record that does not belong to the tag", () => {
    // The workflow used to copy the record into a signed asset without reading
    // it. A tree published for one version and tagged as another would then
    // produce a *signed* assertion that the two correspond — the one claim
    // nobody would afterwards think to question.
    const check = workflow.indexOf('recorded" != "$version');
    expect(check, "the workflow signs the source record without checking it").toBeGreaterThan(-1);
    const signs = workflow.indexOf("shasum -a 256 PROVENANCE.txt >> SHA256SUMS.txt");
    expect(check, "the version is checked after the record is already signed").toBeLessThan(signs);
  });

  it("stays a pure function of the commit and the files", () => {
    // The record ships inside the published tree, and the check it supports is
    // "re-run the publisher and compare". Anything that varies between runs —
    // a timestamp, a hostname, a run number — makes that comparison impossible
    // and quietly retires the strongest check this project has.
    const at = publisher.indexOf("const provenance = {");
    const body = publisher.slice(at, publisher.indexOf("};", at));
    for (const varying of ["Date", "hostname", "now()", "env."]) {
      expect(
        body,
        `the record includes ${varying}, so two publishes of one commit differ`,
      ).not.toContain(varying);
    }
  });

  it("is bound to the release by the signature people already check", () => {
    // PROVENANCE.txt names what only the run knows; SHA256SUMS.txt covers it;
    // minisign covers SHA256SUMS.txt. One signature, whole chain.
    expect(workflow).toMatch(/PROVENANCE\.txt/);
    const sums = workflow.indexOf("shasum -a 256 PROVENANCE.txt >> SHA256SUMS.txt");
    expect(sums, "the provenance is not covered by the signed checksum list").toBeGreaterThan(-1);
    const signing = workflow.indexOf("minisign -S -s /tmp/minisign.key");
    expect(sums, "the checksums are signed before the provenance is added").toBeLessThan(signing);
    expect(workflow).toMatch(/release-assets\/PROVENANCE\.txt/);
  });

  it("records the public commit and the run, which only the run knows", () => {
    const at = workflow.indexOf("Sovatela release provenance");
    const body = workflow.slice(at, at + 900);
    expect(body).toContain("GITHUB_SHA");
    expect(body).toContain("GITHUB_RUN_ID");
    expect(body).toContain("PROVENANCE.json");
  });
});
