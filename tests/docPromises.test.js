import { describe, it, expect } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "..");
const read = (f) => readFileSync(join(repo, f), "utf8");

// Paragraphs, with their internal line breaks flattened.
//
// Every sweep in this file matches a phrase against a paragraph, and these
// documents are hard-wrapped at about 80 columns — so a phrase lands across a
// line break as often as not, and a regex written as one line silently never
// matches. That is not hypothetical: SECURITY.md said the terminal defects were
// found "before the feature\n  had reached anyone" and the sweep written to
// catch exactly that claim reported nothing, because the wrap fell between
// "feature" and "had". The claim was on the live site for three days.
//
// So paragraphs are flattened before matching, everywhere, without exception.
const paragraphs = (text) =>
  text.split(/\n\s*\n/).map((p) => p.replace(/\s+/g, " ").trim());

// A paragraph is allowed to quote a claim if it is recording that the claim was
// made and was wrong. One list, used by every sweep — two lists drift, and a
// sweep whose allowlist is narrower than the prose people actually write fails
// on honest corrections until someone loosens it in a hurry.
//
// Word forms, not exact words: "withdrawal" is as much a correction as
// "withdrawn", and "previously said" as much as "earlier draft".
const CORRECTION =
  /withdr[ae]w|corrected|does not hold|was wrong|were wrong|no longer|previously said|used to|earlier (?:draft|revision|version)|was false|through 1\./i;

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
      for (const paragraph of paragraphs(doc)) {
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
    const isCorrection = new RegExp(`${CORRECTION.source}|narrower`, "i");

    const offences = [];
    for (const f of tracked) {
      if (!existsSync(join(repo, f))) continue; // withheld from the public mirror
      const text = read(f);
      for (const paragraph of paragraphs(text)) {
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
      for (const paragraph of paragraphs(read(f))) {
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

  // The launch-call sweep looks for claims about the network. This one looks
  // for claims about *who was affected*, which is a different sentence and is
  // why SECURITY.md went on saying the terminal defects were "found before the
  // feature had reached anyone" for three days after the security note withdrew
  // exactly that argument — on the live /security page, next to an advisory
  // saying the affected population is unknown.
  //
  // The register, the note and the advisory all say the same thing: nobody
  // knows who installed it. Any document that says otherwise is wrong.
  it("no document claims the terminal defects reached nobody", () => {
    const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: repo, encoding: "utf8" })
      .split("\0")
      .filter((f) => /\.(md|svelte|html)$/.test(f))
      .filter((f) => f !== "tests/docPromises.test.js");

    // Assertions that nobody was exposed, in the shapes that have been used.
    const claims = [
      /before the feature had reached anyone/i,
      /reached no users/i,
      /affected (?:nobody|no one|no-one)/i,
      /nobody (?:was|had been) affected/i,
      /no ?body installed it/i,
    ];
    // A paragraph may quote the claim in order to withdraw it.
    const withdrawing = CORRECTION;

    const offences = [];
    for (const f of tracked) {
      if (!existsSync(join(repo, f))) continue; // withheld from the public mirror
      for (const paragraph of paragraphs(read(f))) {
        if (!claims.some((c) => c.test(paragraph))) continue;
        if (withdrawing.test(paragraph)) continue;
        offences.push(`${f}: ${paragraph.trim().slice(0, 140)}`);
      }
    }
    expect(
      offences,
      "these assert that the terminal defects reached nobody. The app has no " +
        "telemetry, so who installed the feature cannot be established — the " +
        "advisory and the register both say so.",
    ).toEqual([]);
  });

  // The same page said an advisory had not been issued. One was.
  it("no document says the terminal finding was not issued as an advisory", () => {
    const security = read("SECURITY.md");
    for (const paragraph of paragraphs(security)) {
      if (!/rather than issued as an\s+advisory/i.test(paragraph)) continue;
      expect(
        paragraph,
        "SECURITY.md says no advisory was issued. GHSA-jpv9-3mvc-5v5c exists.",
      ).toMatch(/previously said|corrected|withdraw/i);
    }
    expect(security, "SECURITY.md no longer names the advisory").toContain(
      "GHSA-jpv9-3mvc-5v5c",
    );
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
      // Not a bare "the feature is off": PRIVACY.md says it is "off until you
      // install it", which is true and is about the feature being opt-in
      // rather than about a release having withdrawn it. Flattening paragraphs
      // made this pattern reach that sentence for the first time, so the false
      // positive it had always had finally showed up.
      /the feature is off\b(?!\s+until)/i,
      /before (?:the feature|it) (?:had )?reached anyone/i,
      /reached no users/i,
    ];
    for (const d of docs) {
      const text = read(d);
      for (const paragraph of paragraphs(text)) {
        for (const claim of forbidden) {
          if (!claim.test(paragraph)) continue;
          // Allowed only where the paragraph is recording that the claim was
          // made and was wrong.
          // This list used to be four exact phrases, and the sweep was blind
          // anyway: the claim it exists to catch was wrapped across a line
          // break, so it never matched at all until paragraphs were flattened.
          expect(
            paragraph,
            `${d} still asserts that a released version withdrew terminal access`,
          ).toMatch(CORRECTION);
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

// The opt-in launch update check is the only setting in the app that adds an
// automatic network call. Two properties have to hold for the privacy story to
// stay true, and neither is obvious from reading one file.
describe("the update check at launch stays opt-in", () => {
  const rust = read("src-tauri/src/lib.rs");
  const app = read("src/App.svelte");

  // `#[serde(default)]` on a bool is false. If someone "helpfully" makes it
  // default_true, every existing install starts making a call its owner never
  // agreed to — and the privacy policy becomes wrong for everyone at once.
  it("defaults to off", () => {
    const decl = rust.match(
      /#\[serde\(default(?:\s*=\s*"([a-z_]+)")?\)\]\s*\n\s*check_updates_on_launch: bool/,
    );
    expect(decl, "check_updates_on_launch is gone from AppSettings").toBeTruthy();
    expect(
      decl[1],
      "the launch update check now defaults to on — nobody opted in to that",
    ).toBeUndefined();
  });

  it("does not run unless the setting says so", () => {
    expect(app, "the launch check is gone").toMatch(/checkForUpdateOnLaunch/);
    const fn = app.slice(app.indexOf("async function checkForUpdateOnLaunch"));
    const body = fn.slice(0, fn.indexOf("\n  }"));
    expect(
      body,
      "the launch check no longer reads the setting before calling out",
    ).toMatch(/get_update_check_on_launch/);
    // The guard must come before the request, not after it.
    expect(body.indexOf("get_update_check_on_launch")).toBeLessThan(
      body.indexOf("check_for_update"),
    );
  });

  // Both documents that enumerate what the app contacts must describe it, and
  // must not go back to calling the connection check the only automatic call.
  it("is disclosed in both documents that list network activity", () => {
    for (const [name, doc] of [
      ["SECURITY.md", read("SECURITY.md")],
      ["docs/PRIVACY.md", read("docs/PRIVACY.md")],
    ]) {
      expect(doc, `${name} does not mention the launch update check`).toMatch(
        /(?:only if you switch it on|switch it on|starts.*off by default|off until you enable)/i,
      );
      for (const paragraph of paragraphs(doc)) {
        if (!/only call the app makes on its own/i.test(paragraph)) continue;
        expect(
          paragraph,
          `${name} still calls the connection check the only automatic call, ` +
            "but the opt-in update check can be a second one",
        ).toMatch(CORRECTION);
      }
    }
  });
});

// "Uninstall and data deletion" is the page a person reads when they want
// everything gone — before selling the machine, or after deciding they are
// done with this app. A file it does not name survives a removal the reader
// believes was complete, and they have no way to discover it.
describe("the uninstall page accounts for everything stored", () => {
  const rust = read("src-tauri/src/lib.rs");
  const uninstall = read("docs/UNINSTALL.md");

  // Each of these is written by the app and holds something of the user's.
  it("names every stored file and folder", () => {
    for (const artefact of [
      "settings.json",
      "memories.json",
      "usage.json",
      "conversations/",
      "projects/",
      "compactions/",
      "templates/", // the user's own documents, copied in
    ]) {
      expect(
        uninstall,
        `UNINSTALL.md never mentions ${artefact}, so it survives a removal ` +
          "the reader thinks was complete",
      ).toContain(artefact);
    }
  });

  // The one that was actually wrong. Everything lives under app_config_dir()
  // except templates, which are app_data_dir() — the same folder on macOS and
  // Windows, a different one on Linux (~/.local/share vs ~/.config). The page
  // listed only the config path, so a Linux user who followed it kept a copy
  // of every document template they had supplied.
  it("gives the Linux data directory when anything is stored there", () => {
    if (!/app_data_dir\(\)/.test(rust)) return; // nothing stored there any more
    // The identifier has to be on the path. The page already mentions
    // ~/.local/share/uv, from the terminal-access cleanup, so matching the
    // bare directory would have passed while this app's own data sat there
    // undocumented — which is the state this test was written to catch.
    expect(
      uninstall,
      "something is stored under app_data_dir(), which on Linux is " +
        "~/.local/share/com.anaubi.sovatela — a path UNINSTALL.md does not " +
        "give, so removing the documented folder leaves it behind",
    ).toMatch(/\.local\/share\/com\.anaubi\.sovatela/);
  });
});

// Documents describing a capability the app does not hold.
//
// TECHNICAL-SPEC said the renderer holds `opener:default` for several releases
// after 1.6.2 removed it — while SECURITY.md described the removal. Two
// published documents disagreed about a security control, and nothing noticed,
// because no test compared either of them against the capability file that
// decides the answer.
describe("documents match the capabilities the app actually declares", () => {
  const caps = JSON.parse(read("src-tauri/capabilities/default.json"));
  const held = caps.permissions ?? [];

  const flat = (f) =>
    read(f)
      .replace(/<!--[\s\S]*?-->/g, " ")
      .replace(/\s+/g, " ");

  // A document may name a permission the app no longer holds only while
  // recording that it was removed — the same allowance every other sweep here
  // makes for a correction.
  const RECORDS_REMOVAL =
    /was removed|no longer holds|is not any more|removed in 1\.|used to|until 1\./i;

  it("no document claims a permission that is not in the capability file", () => {
    for (const perm of ["opener:default", "fs:default", "shell:default", "http:default"]) {
      if (held.includes(perm)) continue; // genuinely held — nothing to check
      for (const f of ["docs/TECHNICAL-SPEC.md", "SECURITY.md", "docs/PRIVACY.md"]) {
        for (const sentence of flat(f).split(/(?<=\.)\s+/)) {
          if (!sentence.includes(perm)) continue;
          expect(
            sentence,
            `${f} says the app holds ${perm}, which is not in capabilities/default.json`,
          ).toMatch(RECORDS_REMOVAL);
        }
      }
    }
  });

  // And the converse, so the list cannot be quietly widened without the
  // documents that describe it being updated.
  it("the capability file holds only what is expected", () => {
    expect(
      held.sort(),
      "the main window's permissions changed — TECHNICAL-SPEC § Tauri capabilities " +
        "and SECURITY.md describe this list and must be updated with it",
    ).toEqual(["core:default", "dialog:default"]);
  });
});

// Turning the workspace off is a security decision, so the interface must not
// report it before the backend has done it. This one was missed when the other
// console-only failures were fixed, and it is the worst of the set: the others
// misreport a preference, this one misreported a control while the agent could
// still read and write the folder.
describe("revoking workspace access cannot report success it did not get", () => {
  const keypage = read("src/lib/KeyPage.svelte");

  it("clears the displayed folder only after the backend confirms", () => {
    const at = keypage.indexOf("async function clearWorkspaceFolder");
    expect(at, "clearWorkspaceFolder is gone").toBeGreaterThan(-1);
    const body = keypage.slice(at, keypage.indexOf("\n  }", at));
    expect(body, "the revocation failure is logged rather than shown").not.toMatch(
      /console\.error/,
    );
    expect(body, "nothing tells the user the revocation failed").toMatch(
      /workspaceError\s*=/,
    );
    // The assignment that empties the field must come after the await, not
    // before it.
    const invokeAt = body.indexOf('invoke("clear_workspace_dir")');
    const clearAt = body.indexOf('workspaceDir = ""');
    expect(invokeAt, "the clear command is gone").toBeGreaterThan(-1);
    expect(
      clearAt,
      "the folder is cleared in the interface before the backend confirms it",
    ).toBeGreaterThan(invokeAt);
  });
});

// A role is a promise about behaviour. modalFocus.js exists because the
// project made this mistake once already, and its comment says so.
describe("nothing claims to be a dialog without behaving like one", () => {
  const app = read("src/App.svelte");

  // Comments stripped first. The second time this file has needed that: a
  // comment explaining why a claim is not made contains the claim, and a guard
  // that reads comments as markup fires on the explanation.
  const markup = app
    .replace(/<!--[\s\S]*?-->/g, " ")
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    .replace(/(^|[^:])\/\/.*$/gm, "$1 ");

  it("the update prompt does not declare a role it does not implement", () => {
    if (!/role="dialog"|aria-modal/.test(markup)) return; // nothing claims it
    expect(
      markup,
      "App.svelte declares a dialog but never uses the modalFocus action, so " +
        "focus is never moved, trapped or restored",
    ).toMatch(/use:modalFocus/);
  });
});

// The update check is the one network call a user is asked to opt into, so the
// consent copy is held to the standard the policy documents already meet: no
// application data is sent, and the request itself is still a request.
describe("the update-check copy does not overstate", () => {
  const flat = (f) => read(f).replace(/<[^>]*>/g, " ").replace(/\s+/g, " ");

  // The documents too — and that omission is why this guard passed while
  // SECURITY.md's own table said the update check sends "Nothing. No query
  // string, nothing about you or your machine", live on the site, after the
  // same claim had been narrowed in all three places in the app. A guard that
  // sweeps the code and not the page it is published on is half a guard.
  it("does not claim the request sends nothing at all", () => {
    for (const f of [
      "src/App.svelte",
      "src/lib/KeyPage.svelte",
      "SECURITY.md",
      "docs/PRIVACY.md",
      "docs/QUICKSTART.md",
      "docs/FAQ.md",
      "docs/TECHNICAL-SPEC.md",
    ]) {
      expect(
        flat(f),
        `${f} says the update check "sends nothing", which is not true of the ` +
          "request itself — GitHub hosts the page and sees the IP",
      ).not.toMatch(
        /sends nothing[.,—]|sends nothing\s+—\s+no|the same: nothing|\| Nothing\. No query string/i,
      );
    }
  });

  // Found by launching the packaged app and looking at it: `button.ghost`
  // borders itself with --border, which is within a few percent of this
  // banner's tinted background, so "No" rendered as bare text beside a solid
  // primary. Refusing must not look unavailable in a consent prompt.
  it("makes declining look like a button", () => {
    const app = read("src/App.svelte");
    const at = app.indexOf(".ask-updates-actions");
    expect(at, "the prompt's action row is gone").toBeGreaterThan(-1);
    expect(
      app.slice(at),
      "the decline button has no border override, so it inherits --border and " +
        "disappears against the banner's own background",
    ).toMatch(/button\.ghost\)?\s*\{[^}]*border-color/);
  });

  it("says who sees the request instead", () => {
    expect(
      flat("src/App.svelte"),
      "the prompt no longer discloses that GitHub sees the request",
    ).toMatch(/GitHub[^.]{0,60}(sees|hosts)/i);
  });
});

// The badge, and the key-expiry advice that depends on it. Both are the same
// bargain: the app asks people to accept an interruption it can explain, so it
// has to actually explain it.
describe("an update people did not ask for still reaches them", () => {
  const app = read("src/App.svelte");
  const chat = read("src/lib/Chat.svelte");
  const keypage = read("src/lib/KeyPage.svelte");

  // The banner is dismissible and gone for the session. If that were the only
  // notice, dismissing it once would be the same as never being told.
  it("survives the banner being dismissed", () => {
    expect(app, "the persistent update state is gone").toMatch(/updateAvailable/);
    expect(app, "the badge state is not passed to the chat view").toMatch(
      /<Chat[\s\S]{0,200}\{updateAvailable\}/,
    );
    const dismiss = app.slice(app.indexOf("update-banner-close"));
    expect(
      dismiss.slice(0, 200),
      "dismissing the banner also clears the badge",
    ).not.toMatch(/updateAvailable\s*=\s*null/);
  });

  it("shows beside Settings, and says so in words as well as colour", () => {
    expect(chat, "the badge is gone from the header").toMatch(/update-dot/);
    expect(
      chat,
      "the badge is colour only — nothing announces it",
    ).toMatch(/sr-only[\s\S]{0,200}is available/);
  });

  // The setting has a consequence, and the consequence is the reason to turn it
  // on. Describing the feature without it leaves people declining a security
  // notice they did not know they were declining.
  it("says what declining it costs", () => {
    const flat = keypage.replace(/\s+/g, " ");
    expect(
      flat,
      "the update setting no longer says that security fixes will not reach you",
    ).toMatch(/you will not hear about security fixes/i);
    expect(flat, "it no longer says updating stays manual").toMatch(
      /nothing installs itself|updating is still manual/i,
    );
  });
});

// A key with an expiry date is the app's own recommendation, so a key that has
// expired has to be explained by the app rather than discovered by the user.
// The previous advice was the opposite — "choose Never" — and its stated reason
// was that the error would not mention the key. That reason no longer holds.
describe("an expired key explains itself", () => {
  const chat = read("src/lib/Chat.svelte");
  const steps = read("src/lib/ScalewayKeySteps.svelte");
  const quickstart = read("docs/QUICKSTART.md");
  const troubleshooting = read("docs/TROUBLESHOOTING.md");

  // Comments are stripped first. The negative assertion below is about what a
  // user is shown, and the comment beside the code quotes the very phrase it
  // exists to forbid — which the first version of this test duly caught, in the
  // explanation rather than in the copy.
  const withoutComments = (src) =>
    src
      .replace(/<!--[\s\S]*?-->/g, " ")
      .replace(/\/\*[\s\S]*?\*\//g, " ")
      .replace(/(^|[^:])\/\/.*$/gm, "$1 ")
      .replace(/\s+/g, " ");

  it("names expiry as a cause when Scaleway refuses the key", () => {
    const copy = withoutComments(chat);
    expect(copy, "the rejected-key state no longer mentions expiry").toMatch(
      /expir/i,
    );
    // And it must not assert expiry, because a 401 cannot distinguish it from a
    // revoked or mistyped key.
    expect(
      copy,
      "the app states the key has expired, which Scaleway's answer cannot tell it",
    ).not.toMatch(/your key has expired|the key has expired\b/i);
  });

  // The setup copy is where this slipped: "Sovatela tells you when it lapses"
  // and "will tell you why" both claim the app detects expiry. It detects a
  // refusal. The expiry date lives in the Scaleway account and is not part of
  // the key, so the app never sees it and cannot warn ahead of the day.
  it("never claims the app can detect or foresee expiry", () => {
    for (const f of ["src/lib/KeyPage.svelte", "src/lib/ScalewayKeySteps.svelte", "src/lib/Chat.svelte"]) {
      const copy = withoutComments(read(f));
      // Affirmative claims only. "cannot warn you in advance" is the honest
      // sentence and contains the same words as the claim it denies, so a
      // pattern that ignores the negation fires on the correction — which is
      // what happened when this was first written.
      const claims = [
        /tells? you when it (?:lapses|expires)/gi,
        /will tell you why/gi,
        /warns? you (?:before|in advance|when it expires)/gi,
        /lets? you know when (?:it|the key) (?:lapses|expires)/gi,
      ];
      const NEGATED = /(?:cannot|can't|does not|doesn't|will not|won't|never|no way to)\s+(?:\w+\s+){0,3}$/i;
      for (const claim of claims) {
        for (const m of copy.matchAll(claim)) {
          const before = copy.slice(Math.max(0, m.index - 40), m.index);
          if (NEGATED.test(before)) continue; // a denial, not a promise
          expect.fail(
            `${f} says the app knows the key expired — "${m[0]}". It only ` +
              "knows the key was refused, and cannot tell expiry from a " +
              "revoked or mistyped one",
          );
        }
      }
    }
  });

  it("shows that state where the user is, not only as a tooltip", () => {
    expect(
      chat,
      "a refused key is signalled by the status dot alone",
    ).toMatch(/connState === "auth"/);
  });

  it("recommends an expiry date, and offers the way out", () => {
    const flat = steps.replace(/\s+/g, " ");
    expect(flat, "the setup steps no longer suggest an expiry date").toMatch(
      /expiry date|Expiration<\/strong> — set a date/i,
    );
    expect(
      flat,
      "the setup steps no longer tell people they may choose Never instead",
    ).toMatch(/Never/);
    expect(
      flat,
      "the steps still tell people to choose Never as the default",
    ).not.toMatch(/Expiration<\/strong> — choose <strong>Never/);
  });

  it("documents renewal where someone lands when chat stops", () => {
    for (const [name, doc] of [
      ["QUICKSTART.md", quickstart],
      ["TROUBLESHOOTING.md", troubleshooting],
    ]) {
      const flat = doc.replace(/\s+/g, " ");
      expect(flat, `${name} does not explain an expired key`).toMatch(/expir/i);
      expect(flat, `${name} does not say how to renew`).toMatch(
        /iam\/api-keys|IAM → API keys/i,
      );
    }
  });
});

// A save that fails and says nothing is worse than one that fails loudly: the
// user believes the thing happened. The sharpest case was removing the key —
// the screen changed to the welcome page whether or not the keychain accepted
// the deletion, so "my key is gone" was what a failure looked like.
describe("settings failures reach the user, not the console", () => {
  const app = read("src/App.svelte");
  const keypage = read("src/lib/KeyPage.svelte");

  it("a failed key removal does not show the welcome screen", () => {
    const fn = app.slice(app.indexOf("async function removeKey"));
    const body = fn.slice(0, fn.indexOf("\n  }"));
    expect(body, "the key removal catch is gone").toMatch(/catch/);
    const cat = body.slice(body.indexOf("catch"));
    expect(
      cat,
      "removing the key fails silently again — it must not log and carry on",
    ).not.toMatch(/console\.error/);
    expect(cat, "the failure is not surfaced to the user").toMatch(/keyRemovalError/);
    // The navigation must not happen on the failure path.
    expect(
      cat.slice(0, cat.indexOf("}")),
      "the welcome screen is still shown after a failed removal",
    ).not.toMatch(/view\s*=\s*"welcome"/);
  });

  it("is rendered with an alert role so a screen reader announces it", () => {
    expect(app).toMatch(/role="alert"[\s\S]{0,80}keyRemovalError|keyRemovalError[\s\S]{0,120}role="alert"/);
  });

  // The settings saves that used to log and leave the button unchanged, which
  // is indistinguishable from a slow save.
  it("each settings save surfaces its own failure", () => {
    for (const [fn, state] of [
      ["saveSearch", "searchError"],
      ["saveImage", "imageError"],
      ["saveTerminalKey", "terminalKeyError"],
      ["clearTerminalKey", "terminalKeyError"],
      ["saveMemory", "memoryError"],
    ]) {
      const at = keypage.indexOf(`async function ${fn}(`);
      expect(at, `${fn} is gone`).toBeGreaterThan(-1);
      const body = keypage.slice(at, keypage.indexOf("\n  }", at));
      const cat = body.slice(body.indexOf("catch"));
      expect(cat, `${fn} logs its failure instead of showing it`).not.toMatch(
        /console\.error/,
      );
      expect(cat, `${fn} does not set ${state}`).toMatch(new RegExp(state));
    }
    // And each one is actually rendered somewhere, with an alert role.
    for (const state of ["searchError", "imageError", "terminalKeyError", "memoryError"]) {
      expect(
        keypage,
        `${state} is set but never rendered`,
      ).toMatch(new RegExp(`\\{#if ${state}\\}[\\s\\S]{0,120}role="alert"`));
    }
  });
});

// Auto-memory is the app's other opt-in, and the one whose default decides
// whether personal facts start being collected. The stored setting was off by
// default and correct; the two values that *write* it were not, which is how a
// documented opt-in becomes an opt-out without any document changing.
describe("auto-memory stays opt-in", () => {
  const rust = read("src-tauri/src/lib.rs");
  const keypage = read("src/lib/KeyPage.svelte");

  // The stored setting. Same shape as the update-check guard above.
  it("defaults to off where it is stored", () => {
    const decl = rust.match(
      /#\[serde\(default(?:\s*=\s*"([a-z_]+)")?\)\]\s*\n\s*auto_memory: bool, \/\/ suggest/,
    );
    expect(decl, "auto_memory is gone from AppSettings").toBeTruthy();
    expect(
      decl[1],
      "auto-memory now defaults to on — nobody opted in to that",
    ).toBeUndefined();
  });

  // The transfer struct. This one is deserialized from the renderer and
  // written straight into settings, so `default_true` here means a payload
  // that omits the field switches the feature on.
  it("cannot be switched on by a payload that omits it", () => {
    const dto = rust.slice(rust.indexOf("struct MemorySettings"));
    const decl = dto
      .slice(0, dto.indexOf("}"))
      .match(/#\[serde\(default(?:\s*=\s*"([a-z_]+)")?\)\]\s*\n\s*auto_memory: bool/);
    expect(decl, "auto_memory is gone from MemorySettings").toBeTruthy();
    expect(
      decl[1],
      "MemorySettings.auto_memory defaults to on, so a partial payload enables it",
    ).toBeUndefined();
  });

  // The interface's own starting value, which is what Save sends. Rendering an
  // on toggle to someone who has the feature off is how they end up enabling
  // it by saving something else on the same panel.
  //
  // This sweeps every component rather than the one file the fix was made in.
  // The first version of this test checked KeyPage alone, passed, and was
  // quoted in the release notes as holding the property — while Chat.svelte
  // still initialised the same flag to true, and that copy is the one gating
  // the billed extraction request. A guard that names its file only guards
  // that file.
  it("starts off everywhere it is declared", () => {
    const files = execFileSync("git", ["ls-files", "-z", "src"], { cwd: repo, encoding: "utf8" })
      .split("\0")
      .filter((f) => /\.(svelte|js)$/.test(f));
    const declaring = [];
    for (const f of files) {
      const src = read(f);
      for (const m of src.matchAll(/let autoMemory\s*=\s*\$state\((true|false)\)/g)) {
        declaring.push(f);
        expect(
          m[1],
          `${f} starts auto-memory on, before the stored value has been read`,
        ).toBe("false");
      }
    }
    expect(declaring.length, "no component declares autoMemory any more").toBeGreaterThan(1);
  });

  // False also means "not read yet", so the component that acts on the setting
  // must know the difference. Without this, a settings read that never returns
  // is indistinguishable from a user who turned the feature off — which is the
  // safe direction here, but only by accident.
  it("does not act on the setting before it has been read", () => {
    const chat = read("src/lib/Chat.svelte");
    expect(chat, "the loaded flag is gone").toMatch(/autoMemoryLoaded/);
    const fn = chat.slice(chat.indexOf("function wrapUpMemory"));
    const guard = fn.slice(0, fn.indexOf("\n  }"));
    expect(
      guard,
      "the memory scan runs without confirming the setting was loaded",
    ).toMatch(/!autoMemoryLoaded/);
  });
});
