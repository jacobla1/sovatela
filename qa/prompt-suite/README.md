# Prompt suite

61 prompts across 10 categories, driven through the app's own send path against
the real Scaleway GLM-5.2 endpoint — graded where grading is possible, and
measured for latency, tokens and cost throughout.

```sh
npm run qa:prompts                       # collect, then grade
QA_ONLY=1,2,7 npm run qa:prompts         # categories 1, 2 and 7
QA_CASE=6.4   npm run qa:prompts         # one case
npm run qa:score                         # re-grade the newest run, sends nothing
```

Keys come from the environment, the same way `scripts/run-integration-tests.sh`
does it: `SCALEWAY_API_KEY`, optionally from a gitignored `.env.integration`.

## It goes through the app, not around it

The replies are produced by **the app's own send path** — the `qa_prompt_suite`
test in `src-tauri/src/lib.rs` calls `run_chat` against a Tauri `mock_app`,
exactly as `send_chat` does when you press Enter in the composer.

That matters more than it sounds. The app prepends a system prompt, compacts
long payloads, offers tools, and books tokens into the usage ledger. Calling
Scaleway directly would have been far less work and would have measured a
request the app never sends — including token counts that are a re-derivation
rather than what the app actually charges you for.

The suite is split in two for that reason: Rust does the transport, because
that is where the app's behaviour lives, and Node does the grading, because
then the checks can be unit-tested without spending anything.

The collector is **not** named `integ_*`. `scripts/run-integration-tests.sh`
runs everything matching that prefix, and a routine integration run should not
silently fire 61 billed prompts.

**This bills your Scaleway account.** A measured full run cost **€0.42** and took
**18 minutes** (2026-08-06). The cost is dominated by reasoning tokens, not by
reply length: the simplest factual question billed 86 output tokens for an
8-token answer. `QA_CASE` runs one case if you just want the shape of the output.

## What PASS means, and what it does not

A case reports **PASS** only when every machine-checkable assertion held. That
is a narrow claim: 1.5 passing means the reply contained "206", not that the
answer was good.

A case with nothing machine-checkable reports **REVIEW**, never PASS. "Is the
poem coherent", "are the two voices distinct", "is that regex reasonable" — a
runner that scored those green would be manufacturing confidence, and a green
suite that means nothing is worse than no suite. Seven cases are REVIEW by
design, and the report lists each with the question a human has to answer.

Refusal and clarification detection are keyword **heuristics**. They are marked
as such in the report and collected into their own section, because a refusal
phrased unusually reads as a non-refusal here. Category 7 always deserves a
glance regardless of its verdict.

## What it does not cover

It talks to the API directly, so it tests the model and the cost, not the app.
Anything about the composer, rendering, or the webview has to be checked in the
app — cases 10.1 and 10.2 are marked `appLevel` for that reason: sending an
empty string to the API is a different test from an empty composer, and only the
second one is interesting.

In particular, 10.6 and 10.7 (a script tag and a SQL string) test that the
*model* discusses them rather than obeying them. Whether the *app* renders a
reply safely is a separate question, answered by the CSP and the sanitiser, not
here.

## Files

| | |
| --- | --- |
| `prompts.json` | The suite. Cases carry `assert` (machine-checkable), `review` (a question for a human), or both |
| `assertions.mjs` | The checkers, kept separate so they can be unit-tested without sending a request — see `tests/promptSuite.test.js` |
| `run.sh` | Collect then grade — the normal entry point |
| `score.mjs` | Grades a collected run and writes the report. Never contacts a provider |
| `qa_prompt_suite` in `src-tauri/src/lib.rs` | The collector: drives `run_chat` and writes `raw-*.jsonl` |
| `results/` | `raw-*.jsonl` from the collector and `report-*.md` from the grader. Gitignored — the raw files contain full model replies |

## Adding a case

Give it an `assert` if a machine can genuinely decide it, a `review` note if it
cannot, and both when the machine check is necessary but not sufficient — 2.6
asserts the answer contains "7" and asks a human to read the proof, because 7
can appear by luck.

A case with neither is a test that silently passes, so `tests/promptSuite.test.js`
fails the build if one exists.
