import { describe, it, expect } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// releaseHygiene checks that every document agrees about the version number.
// Nothing checked that they agreed about behaviour, and they did not: the
// privacy policy said no network call happens on its own while the app has
// always checked the connection at launch, and Quick Start said Word headers
// and footers are never sent while they have been read since 1.5.5.
//
// These are behavioural claims tied to the code that makes them true or false,
// so each one asserts the code first and the prose second. A test that only
// grepped the prose would pass just as happily after the behaviour changed.
describe("high-risk promises match the implementation", () => {
  const chat = read("src/lib/Chat.svelte");
  const rust = read("src-tauri/src/lib.rs");
  const privacy = read("docs/PRIVACY.md");
  const security = read("SECURITY.md");
  const quickstart = read("docs/QUICKSTART.md");

  it("the launch connection check is disclosed wherever network activity is listed", () => {
    // The behaviour: checkConnection is invoked as a bare statement while the
    // component initialises, not from a handler. That is what makes it the one
    // call the app places without being asked.
    expect(chat).toMatch(/invoke\("check_connection"\)/);
    expect(
      chat,
      "check_connection is no longer called automatically — the disclosures below now overstate what the app does",
    ).toMatch(/^ {2}checkConnection\(\);$/m);

    // Both documents that enumerate what the app contacts have to say so.
    for (const [name, doc] of [
      ["SECURITY.md", security],
      ["docs/PRIVACY.md", privacy],
    ]) {
      expect(doc, `${name} does not disclose the automatic launch call`).toMatch(
        /automatic/i,
      );
      expect(doc, `${name} does not name the endpoint it calls`).toContain("/models");
    }
  });

  it("no document still claims the app contacts nothing on its own", () => {
    // Both documents keep a note saying the claim used to be there and was
    // wrong, which is worth more than deleting it silently — so the phrase is
    // allowed only in a paragraph that says so.
    const claims = [
      /[Nn]othing runs in the background/,
      /none of the (three|two|four) above happens\s+on its own/,
    ];
    for (const [name, doc] of [
      ["SECURITY.md", security],
      ["docs/PRIVACY.md", privacy],
      ["docs/QUICKSTART.md", quickstart],
    ]) {
      for (const paragraph of doc.split(/\n\s*\n/)) {
        for (const claim of claims) {
          if (!claim.test(paragraph)) continue;
          expect(
            paragraph,
            `${name} still states, as fact, that the app contacts nothing on its own`,
          ).toMatch(/corrected|used to|through 1\./i);
        }
      }
    }
  });

  // The test above names three documents. That is why it passed while the
  // security note said, twice, that Sovatela has "no background network
  // activity" — a fourth document, and a phrasing neither pattern matched. The
  // claim had already been corrected in SECURITY.md and PRIVACY.md, so the
  // published set disagreed with itself about the one automatic call.
  //
  // A list of documents cannot guard a claim that can be written anywhere. So
  // this walks every tracked document and comment instead, and it is deliberate
  // that it will fire on a file nobody thought to add here.
  it("no tracked file anywhere denies the launch call", () => {
    // Assert the behaviour first: if this ever stops being true, the sweep
    // below is forbidding documents from stating a fact, and must go.
    expect(chat).toMatch(/^ {2}checkConnection\(\);$/m);
    expect(rust).toMatch(/async fn check_connection/);

    const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: repo, encoding: "utf8" })
      .split("\0")
      .filter((f) => /\.(md|svelte|js|mjs|rs|html)$/.test(f))
      // The QA capture record and this test describe the false claims by
      // quoting them; that is their subject matter.
      .filter((f) => f !== "qa/network-capture/README.md" && f !== "tests/docPromises.test.js");

    // Denials of the automatic call, in any wording that has been used or is
    // likely to be. "No automatic updater" is true and is not one of these.
    const denials = [
      /no background network activity/i,
      /no background (?:channel|traffic|connections?|calls?)/i,
      /nothing runs in the background/i,
      /makes? no (?:network )?calls? (?:on launch|at launch|at startup)/i,
      /contacts? nothing (?:on its own|by itself)/i,
      /no (?:network )?activity (?:on|at) (?:launch|startup)/i,
    ];
    // A paragraph is allowed to contain the phrase if it is recording that the
    // phrase was wrong. Deleting the history silently is worth less than this.
    // Word forms, not exact words: a correction record that says "records the
    // withdrawal" or "the narrower true claim" is as much a correction as one
    // that says "withdrawn" or "narrower than". This fired on QA-1.6.2.md,
    // which is a table of claims that were corrected — the guard was right to
    // look and wrong about what counts as looking like a correction.
    const isCorrection =
      /corrected|used to|no longer|earlier (?:draft|revision|version)|withdraw|through 1\.|was wrong|narrower/i;

    const offences = [];
    for (const f of tracked) {
      if (!existsSync(join(repo, f))) continue; // withheld from the public mirror
      const text = read(f);
      for (const paragraph of text.split(/\n\s*\n/)) {
        if (!denials.some((d) => d.test(paragraph))) continue;
        if (isCorrection.test(paragraph)) continue;
        offences.push(`${f}: ${paragraph.trim().slice(0, 120)}`);
      }
    }
    expect(
      offences,
      "these state that the app makes no call on its own. It makes one: " +
        "check_connection runs at launch. Say what is actually true — that " +
        "nothing reaches an installed copy — or record the correction.",
    ).toEqual([]);
  });

  // The accessibility statement, the changelog and the QA record all say the
  // chat-list change is unverified. The published 1.6.1 release notes said it
  // "announces its positions to VoiceOver again", flatly. Same release, two
  // answers, and the flat one is the copy most people read.
  //
  // The markup fix is real and tests/landmarks.test.js holds it. What no test
  // can hold is that a screen reader was pointed at it, so any document making
  // the claim has to carry the qualification with it.
  it("no document reports the chat-list fix as confirmed", () => {
    const accessibility = existsSync(join(repo, "docs/ACCESSIBILITY.md"))
      ? read("docs/ACCESSIBILITY.md")
      : null;
    if (accessibility) {
      expect(
        accessibility,
        "the accessibility statement no longer records the chat list as unverified",
      ).toMatch(/unverified rather than fixed/i);
    }

    const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: repo, encoding: "utf8" })
      .split("\0")
      .filter((f) => /\.md$/.test(f))
      .filter((f) => f !== "tests/docPromises.test.js");

    // A claim that the list announces, reads or works under a screen reader.
    const claim = /chat list[^.]{0,120}(?:announce|reads?|works?)[^.]{0,80}(?:voiceover|screen reader)|(?:voiceover|screen reader)[^.]{0,80}chat list[^.]{0,120}(?:announce|reads?|works?)/i;
    // What has to be nearby for the claim to be honest.
    const qualified = /unverified|not (?:yet )?confirmed|no (?:voiceover|screen-reader|screen reader) pass|should announce|untested|never been tested/i;

    const offences = [];
    for (const f of tracked) {
      if (!existsSync(join(repo, f))) continue; // withheld from the public mirror
      for (const paragraph of read(f).split(/\n\s*\n/)) {
        if (!claim.test(paragraph)) continue;
        if (qualified.test(paragraph)) continue;
        offences.push(`${f}: ${paragraph.trim().slice(0, 120)}`);
      }
    }
    expect(
      offences,
      "these report the chat-list change as confirmed. The cause was found and " +
        "the markup changed; no screen reader has been run against it since.",
    ).toEqual([]);
  });

  it("Quick Start agrees with the extractor about what a Word file gives up", () => {
    // The behaviour: headers, footers and notes are read.
    for (const part of ["word/header", "word/footnotes.xml", "word/endnotes.xml"]) {
      expect(rust, `${part} is no longer extracted`).toContain(part);
    }
    // Comments are still not.
    expect(rust).not.toContain("word/comments.xml");

    expect(
      quickstart,
      "Quick Start still tells the user headers and footers are not sent",
    ).not.toMatch(/headers,?\s*\n?footers,? footnotes and comments are not/);
    expect(quickstart).toMatch(/[Cc]omments and tracked-change deletions\s*\n?\*{0,2}are not/);
  });

  // The register and the security note both claimed v1.6.0 had withdrawn
  // terminal access. It had not: the withdrawal was written after the release,
  // in a working copy that was discarded. A security document asserting a
  // protection that does not exist is the worst version of the F15 defect, and
  // it was committed while responding to F15.
  it("no document claims a released version withdrew terminal access", () => {
    const docs = [
      "docs/release/SECURITY-NOTE-2026-08-30-claude-glm.md",
      "SECURITY.md",
      "docs/TERMS.md",
      "CHANGELOG.md",
      "README.md",
      "docs/PRIVACY.md",
    ];
    const forbidden = [
      /withdrawn in 1\.6\.0/i,
      /the feature is off/i,
      /before (?:the feature|it) (?:had )?reached anyone/i,
      /reached no users/i,
    ];
    for (const d of docs) {
      const text = read(d);
      for (const paragraph of text.split(/\n\s*\n/)) {
        for (const claim of forbidden) {
          if (!claim.test(paragraph)) continue;
          // Allowed only where the paragraph is recording that the claim was
          // made and was wrong.
          expect(
            paragraph,
            `${d} still asserts that a released version withdrew terminal access`,
          ).toMatch(/earlier draft|was false|withdrawn:|does not hold/i);
        }
      }
    }
  });

  it("the register is version-controlled and names the commit it describes", () => {
    // The previous revision lived only as a Desktop export. It was reviewed in a
    // stale state, producing a round of review against claims already withdrawn.
    const reg = read("docs/release/REMEDIATION-REGISTER.md");
    // The commit must be real, an ancestor of HEAD, and NOT HEAD — a register
    // cannot name the commit that contains it, and the previous test only
    // checked that some seven-character hash was present. It named one two
    // commits behind and nothing noticed.
    const named = reg.match(/Code commit described: `([0-9a-f]{7,40})`/)?.[1];
    expect(named, "the register does not name the commit it describes").toBeTruthy();
    const exists = (rev) => {
      try {
        return execFileSync("git", ["rev-parse", "--verify", `${rev}^{commit}`], {
          cwd: repo, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"],
        }).trim();
      } catch {
        return null;
      }
    };
    // CI checks out with fetch-depth 1, so the commit the register names — its
    // own parent — is genuinely not in the local object store. Asserting it
    // exists there fails for a reason that has nothing to do with the register,
    // which is what this test is for. The format, the generator and the row
    // agreement below are checked everywhere; only the git relationship is
    // conditional on git being able to see it.
    //
    // The same is true, permanently, of the public mirror. The register names a
    // commit in the private repository; `deploy/publish-source.mjs` regenerates
    // jacobla1/sovatela as its own history, so that SHA is not in its object
    // store and never will be. Asserting it exists there made the *published*
    // source fail its own suite while passing here — which is precisely the
    // finding the August 2026 audit made about `docs/ACCESSIBILITY.md`, in a
    // second file. A test that can only pass in the repository it was written
    // in is not a check on the release; it is a check on the developer's disk.
    let shallow = true;
    try {
      shallow =
        execFileSync("git", ["rev-parse", "--is-shallow-repository"], {
          cwd: repo, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"],
        }).trim() !== "false";
    } catch {
      shallow = true; // not a git checkout at all
    }

    let isSourceRepo = false;
    try {
      isSourceRepo = /jacobla1\/Scale(\.git)?$/.test(
        execFileSync("git", ["remote", "get-url", "origin"], {
          cwd: repo, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"],
        }).trim(),
      );
    } catch {
      isSourceRepo = false; // no remote: a tarball, or the mirror before its first push
    }

    const canSeeHistory = !shallow && isSourceRepo;
    const resolved = canSeeHistory ? exists(named) : null;
    const head = canSeeHistory ? exists("HEAD") : null;
    if (canSeeHistory) {
      expect(resolved, `the register names ${named}, which is not a commit here`).toBeTruthy();
    }
    if (head && resolved && resolved !== head) {
      let ancestor = false;
      try {
        execFileSync("git", ["merge-base", "--is-ancestor", resolved, "HEAD"], { cwd: repo });
        ancestor = true;
      } catch {
        ancestor = false;
      }
      expect(ancestor, `the register names ${named}, which is not an ancestor of HEAD`).toBe(true);
    }

    // And the generator it claims to come from has to exist.
    expect(existsSync(join(repo, "scripts/gen-remediation-register.mjs"))).toBe(true);

    // The register must match its source rows. This is the drift that actually
    // happened: the register kept saying F17 had no hash verification for two
    // commits after it did. Editing the rows without regenerating now fails.
    const rows = JSON.parse(read("scripts/remediation-rows.json"));
    expect(rows.length).toBeGreaterThan(15);
    for (const row of rows) {
      const [id, ...cells] = row;
      const line = reg
        .split("\n")
        .find((l) => l.startsWith(`| \`${id}\` |`));
      expect(line, `${id} is missing from the generated register`).toBeTruthy();
      for (const cell of cells) {
        expect(
          line,
          `${id} in the register does not match scripts/remediation-rows.json — regenerate it`,
        ).toContain(cell);
      }
    }
    expect(reg).toContain("Generated by `scripts/gen-remediation-register.mjs`");
    expect(reg).toMatch(/Public release under review/);
    // Both status columns, kept apart — conflating them is what produced the
    // false claim about what v1.6.0 shipped.
    expect(reg).toContain("| Public v1.6.0 | Candidate | Residual risk | Evidence |");
    expect(reg).toMatch(/no known\s*\n?\s*exploitation/i);
    // Every finding, plus the one found during remediation.
    for (let n = 1; n <= 20; n++) {
      const id = `F${String(n).padStart(2, "0")}`;
      expect(reg, `${id} is missing from the register`).toContain(`\`${id}\``);
    }
  });

  it("no shipped file claims a package count it did not compute", () => {
    // The lock has ~110 packages and ~2,771 lines. Printing the line count as a
    // package count, or hard-coding either, is the evidence drift this project
    // has been caught by repeatedly — including in the workflow that exists to
    // produce evidence.
    const lock = read("deploy/claude-glm/requirements.lock");
    const packages = (lock.match(/^[a-zA-Z0-9_.-]+==/gm) || []).length;
    expect(packages).toBeGreaterThan(50);
    for (const f of [
      "deploy/claude-glm/install-claude-glm.sh",
      "deploy/claude-glm/install-claude-glm.command",
      "deploy/claude-glm/install-claude-glm.ps1",
      "deploy/claude-glm/verify-linux-e2e.sh",
      ".github/workflows/linux-terminal-access.yml",
      ".github/workflows/windows-terminal-access.yml",
      "src-tauri/src/lib.rs",
    ]) {
      const text = read(f);
      // A literal count next to the word "package" is a claim nothing checks.
      const claims = [...text.matchAll(/(\d{2,5})[- ]?(?:Python )?packages?/g)]
        .map((m) => Number(m[1]))
        .filter((n) => n !== packages);
      expect(claims, `${f} hard-codes a package count: ${claims}`).toEqual([]);
    }
  });

  it("no comment still says a Python is downloaded when it is not", () => {
    for (const f of [
      "deploy/claude-glm/install-claude-glm.sh",
      "deploy/claude-glm/install-claude-glm.command",
      "deploy/claude-glm/install-claude-glm.ps1",
    ]) {
      const text = read(f);
      expect(text, `${f} still permits a managed-Python download`).toContain(
        "--no-python-downloads",
      );
      // And the Python check must come before uv is put on disk.
      const py = text.search(/Checking for Python/);
      const uv = text.search(/Installing uv \(the package installer/);
      expect(py, `${f} no longer preflights Python`).toBeGreaterThan(-1);
      expect(py, `${f} installs uv before checking for Python`).toBeLessThan(uv);
    }
  });

  it("the interface does not tell users the feature was withdrawn", () => {
    expect(read("src/lib/KeyPage.svelte")).not.toMatch(/[Tt]erminal access is withdrawn/);
  });

  it("keeps 'no known exploitation' distinct from 'no exploitation'", () => {
    const note = read("docs/release/SECURITY-NOTE-2026-08-30-claude-glm.md");
    expect(note).toMatch(/no known exploitation/i);
    expect(note).toMatch(/not the same as no exploitation/i);
  });

  it("the text-scaling figure is the one the code implements", () => {
    const sizes = read("src/lib/textSize.js");
    const top = Math.max(
      ...[...sizes.matchAll(/pct:\s*(\d+(?:\.\d+)?)/g)].map((m) => Number(m[1])),
    );
    expect(top).toBe(200);
    for (const [name, doc] of [
      ["README.md", read("README.md")],
      ["docs/QUICKSTART.md", quickstart],
    ]) {
      expect(doc, `${name} understates the maximum text size`).not.toMatch(
        /scal\w+[^.]{0,40}\b150%/,
      );
      expect(doc).toContain("200%");
    }
  });
});
