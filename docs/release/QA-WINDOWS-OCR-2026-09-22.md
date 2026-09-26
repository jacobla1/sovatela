# Windows OCR on a hosted runner — 2026-09-22

## What was measured

`windows-ocr-spike.yml`, run twice on GitHub's `windows-latest`
(Windows Server 2025 Datacenter):

- **`Language.OCR~~~en-US~0.0.1.0` is already `Installed`** before anything is
  added. Every other OCR language capability is `NotPresent`.
- **WinRT reports one recognizer language**, `en-US`, through
  `Windows.Media.Ocr.OcrEngine.AvailableRecognizerLanguages`.
- **The helper reads the scan fixture**: exit 0, with the document's own
  scanned-page preamble and the text `INUOICE 12345` — the misread this bitmap
  is already known to produce on Windows, which is why the fixture asserts
  digits and never the whole phrase.

## What that corrects

`ci.yml` carried this, unmeasured, since the OCR fixture was added:

> *"The runners almost certainly have no OCR language pack, so on Windows the
> expected result is 33 and not 0 … Whether it can actually read a scan still
> needs a machine with a language pack on it."*

The first half is false. The consequence was not cosmetic: `check-mixed.mjs`
accepted exit 33 on Windows unconditionally, so **if Windows OCR had broken,
CI would have stayed green**. The tolerance was written to accommodate a
limitation that did not exist, and it was suppressing a real assertion.

Corrected in `6e52e6f`: `SOVATELA_EXPECT_OCR=1`, set by CI on Windows and
macOS, makes a refusal a failure. It is not set on Linux, which has no engine
by design, and not set for developers, whose Windows machine may genuinely
lack the pack — a real configuration rather than a defect.

## What this does **not** establish

**It is not a real-photo accuracy test, and the standing disclosure stays.**
The fixture is a synthetic bitmap drawn from a 5×7 glyph alphabet in
`qa/ocr/glyphs.mjs` — no font, no renderer, nothing between the test and the
recogniser. It answers "is a recogniser present and does it return text", not
"how accurately does Windows read a photographed document".

So the announcement's sentence remains true as written and must not be
widened on this evidence:

> *"Clean-machine installer lifecycles and Windows OCR accuracy on
> photographed documents remain untested."*

It also says nothing about Intel Windows, other language packs, a clean
machine, or a signed release artifact. One hosted runner image, twice.

## Why it was run

The next confinement step is AppContainer for the document helper, and its
deciding risk is whether WinRT OCR still activates inside a container. A
runner that could not OCR unconfined could not answer that question confined,
so a green CI run would have proved nothing about the part most likely to
break — which is precisely how the macOS profile passed its own checks and
still broke Vision's CVNLP path on two of 210 documents.

**That blocker is now removed.** The same fixture can be run inside an
AppContainer and compared against this result, which gives Windows the
equivalent of the byte-for-byte validation the macOS profile received.

## A correction to the first run

The first attempt failed on two faults of its own, both recorded because the
second run's result depends on them being understood:

- PowerShell has no stdin redirection operator — *"The '<' operator is
  reserved for future use"* — so the scan test never executed at all.
- The WinRT query ran under `pwsh` 7, which cannot resolve
  `[Type, Assembly, ContentType=WindowsRuntime]`. It reported **no** recognizer
  languages for a runner that has the pack installed. Taken at face value that
  is a false negative about the entire question, and it was contradicted only
  by the capability listing in the same job.

Both queries now use Windows PowerShell 5.1, and the helper is fed its binary
PDF through `cmd` redirection rather than a PowerShell pipe that would corrupt
it.

## Reproduction

```
Actions → windows-ocr-spike → Run workflow
```

Manual dispatch only: it probes and may install an OS feature, and has no
business running on every push. The install step is retained even though the
capability turned out to be present already — a future runner image may not
have it, and the step reports rather than assumes.
