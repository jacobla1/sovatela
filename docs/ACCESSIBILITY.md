# Accessibility statement

Applies to: Sovatela v1.9.0 · Last reviewed: 2026-08-29

## Our position

We want Sovatela to be usable by everyone. It currently **is not fully
accessible**, and this statement says where it falls short rather than claiming
otherwise. The gaps below are real and were found by review of the source. Some
have been fixed; others have no date, and this page says which is which rather
than describing everything as "tracked".

**Conformance status: does not conform to WCAG 2.1 level AA.**
What is met and what is still open are both listed below. No independent audit
has been commissioned; treat this as a self-assessment.

This said "partially conformant with WCAG 2.1 level AA", which is not what that
formulation is for. WCAG's conformance requirements are met at a level or they
are not; "partial conformance" is reserved for content outside the author's
control — third-party material, or a language the technology cannot support —
and not for the author's own known gaps. An external review pointed this out.

It was then rewritten as "does not currently conform **fully**", which a second
review caught as the same error in quieter clothes: a page that spends a
paragraph explaining why "partially conformant" is the wrong frame should not
then reach for a qualifier that means it. "Not fully" is read as "mostly", which
is precisely the impression the formulation exists to prevent. The unqualified
sentence is the accurate one, and the lists below are where the detail belongs —
they are more use to someone deciding whether this app will work for them than
an adverb ever was.

## What works today

- **Semantic HTML** with a declared page language (`lang="en"`).
- **Light and dark themes**, following the system setting.
- **Adjustable text size to 200%** — *Settings → Appearance → Text size*. A
  desktop app has no address bar to zoom with, and neither macOS nor Windows
  passes its text-size setting to an app like this one, so the app provides the
  control itself. The spacing scale is in `rem`, so the layout grows with the
  type instead of packing larger text into the same gaps.
- **Reduced motion is respected.** With the system setting on, animation and
  transitions stop. The two status dots that pulse to mean "in progress" become
  unfilled rings rather than simply freezing, so the state they carry survives.
- **Colour contrast meets AA in both themes**, measured rather than assumed.
  Every text and control pair is computed from the palette on each build
  (`tests/contrast.test.js`) and the build fails if one drops below its
  requirement — 4.5:1 for text, 3:1 for the edges of controls. Nine pairs were
  short when this was first measured, including warning text at 2.77:1; each
  was corrected rather than annotated. Two colours had to be split apart to get
  there: the accent, because text on a page and white on a filled button pull
  in opposite directions, and the amber used for warnings, which cannot be one
  value across a light and a dark theme.
- **More contrast if your system asks for it.** With *increase contrast* set in
  your operating system, secondary text and the edges of controls step up well
  past the requirement — to 7.7:1 and 10.6:1 for muted text in the two themes.
  There is no setting to find in the app: this follows the choice you already
  made, as reduced motion does.
- **Both dialogs behave like dialogs.** The project editor and the
  keyboard-shortcuts sheet move focus inside themselves, keep <kbd>Tab</kbd>
  there, close on <kbd>Esc</kbd> without that keystroke also reaching the page
  behind, and hand focus back to whatever opened them. The behaviour lives in
  one place, and a test fails if a new dialog declares `aria-modal` without
  using it — which is how the shortcuts sheet came to lack it in the release
  that added focus management to the other one. Both had declared
  `role="dialog"` and `aria-modal="true"` while doing none of what those
  promise, which tells a screen reader the user has entered a dialog while
  their keyboard is still outside it.

  One limit, because the statement above would otherwise be read as more than
  it is: focus goes back to the control that opened the dialog only if that
  control still exists. Deleting a project removes the row its dialog was
  opened from, and focus is then left where the browser puts it rather than
  being placed somewhere sensible. That is a gap, listed below.
- **A reply is announced once, when it is whole.** The conversation used to be
  a live region, so every token of a streaming answer was read as it arrived
  and a paragraph came out as a stutter of fragments. One polite announcer now
  carries what is worth hearing: what the assistant is doing while it takes a
  while, and then the answer itself, read as a sentence. A reply that finishes
  in a chat you are not reading does not interrupt the one you are. Generated
  code is described — *"html artifact, shown in the panel"* — rather than
  spoken aloud.
- **Structure a screen reader can move through.** Each view has one `main`.
  The chat list is named navigation holding real lists, so it announces "3 of
  12" rather than a run of buttons, and marks which chat is open. Settings is
  seven named sections rather than one long page. The conversation and the
  artifact panel are named regions.
- **Keyboard shortcuts**, with a reference at <kbd>?</kbd> and a *Shortcuts*
  button beside *Guide* — a shortcut reachable only by a key you do not know
  about is not much use. <kbd>⌘</kbd>/<kbd>Ctrl</kbd> with <kbd>K</kbd> for a
  new chat, <kbd>B</kbd> for the chat list, <kbd>/</kbd> for the message box,
  <kbd>,</kbd> for Settings. <kbd>Esc</kbd> closes what is on top, or stops a
  reply. Nothing is bound to a bare letter, because a bare letter is a
  character someone is typing.
- **The welcome screen's wording is text.** Its four step cards used to carry
  their titles and notes composited into the artwork, so that wording alone did
  not grow with the text-size control and did not follow the light theme. The
  artwork is wordless now and the words beside it are ordinary text; the
  pictures are marked decorative, since repeating the same words in alternative
  text would only have them read twice.
- **Visible focus indicators** on interactive controls.
- **Labelled controls** — icon-only buttons carry `aria-label`; toggles expose
  `aria-pressed`; decorative icons are `aria-hidden`.
- **Live regions** announce streaming status changes.
- **Dialogs** use `aria-modal` and close on <kbd>Esc</kbd>.
- **Alternative text** on images: attachments use the file's name, and a
  generated picture is described by the prompt that produced it rather than
  by the words "generated image".
- **Keyboard-operable status dot** — reachable by <kbd>Tab</kbd>, activated with
  <kbd>Enter</kbd> or <kbd>Space</kbd>.

## Known gaps

We consider these defects. They are listed with the standard they engage.

| Gap | Impact | WCAG |
| --- | --- | --- |
| **Focus is managed in dialogs, not elsewhere.** Both dialogs take focus, keep it, and hand it back. Settings, the Guide and the history sidebar are full-screen or inline rather than modal, and move focus only as the browser would — opening the sidebar announces itself but does not take focus. | Moving in and out of a dialog works; moving into a panel does not announce itself beyond that. | 2.4.3 (AA) |
| **The chat list has been changed to fix an announcement fault, and the change has not been confirmed with a screen reader.** A pass with VoiceOver on macOS confirmed the project dialog takes focus and keeps it, and that a streaming reply is read once when it finishes rather than a token at a time. The chat list did **not** announce as the markup intends: the `nav`, list semantics and `aria-current` were all present, and the cause turned out to be that WebKit drops a list item's implicit role when the item is laid out with `display: flex`. That was changed in 1.6.1 and a test now holds the markup and the layout rule together — **but no VoiceOver pass has been run since, so treat the chat list as unverified rather than fixed.** No testing has been done with NVDA, JAWS or Orca, so Windows and Linux behaviour is unverified. | Two paths are known good on one reader; the chat list is changed but unconfirmed; the rest is reasoned from the markup. | 4.1.2 (A) |

## What is scheduled, and what is not

**Nothing below has a date.** This section used to be a list of intentions in
priority order, which read as a commitment. It was not one, and saying so is
better than leaving it to be inferred from how long the list stayed unchanged.

**Done since that list was written:** the chat list's announcement fault was
traced to WebKit dropping a flex item's list role, and the markup was changed in
1.6.1. It is a fix in the sense that the cause was found and removed; it is not a
verified fix, because confirming it needs a screen-reader pass that has not been
run.

**Not scheduled:** screen-reader testing on Windows (NVDA, JAWS) or Linux
(Orca), and a second VoiceOver pass covering the chat list and the paths the
first one did not reach. This is a small project without access to those readers
in a form that would make a pass meaningful, so the honest position is that the
behaviour on those platforms is unknown and will stay unknown until it is
tested. This page will be updated if that changes — including if it changes
because someone tells us what they found.

**Still open, and unchanged:** labelling outside the chat view.

Done in 1.5.3: keyboard shortcuts with an in-app reference; landmark roles and
list structure; the live region retuned to announce a reply once it is whole
rather than every token; text scaling to 200% with a spacing scale in `rem`
that grows with it; contrast measured in both themes, nine shortfalls
corrected, and `prefers-contrast: more` honoured; focus management in the
project dialog; and the welcome cards' wording moved out of the artwork into
text.

Done in 1.2.0: the type scale moved from `px` to `rem` with a text-size control
to drive it, the 10px minimum was raised to 11px, and `prefers-reduced-motion`
is now honoured throughout — including in scripted scrolling, which CSS alone
cannot reach.

No date is promised, because this is a small project and a promised date would
be a guess.

## Compatibility

Sovatela is a desktop application rendering in a system webview (WebKit on
macOS and Linux, WebView2 on Windows). Platform screen readers and system-level
zoom apply. Neither macOS nor Windows passes its own text-size setting through
to an application like this one, which is why the app carries the control
itself — Settings ▸ Appearance ▸ Text size, up to 200%.

## Feedback

If you hit an accessibility barrier, tell us — it will be treated as a bug, not
a request:

**`info@anaubi.com`**

Please include your assistive technology and version, your OS, and what you were
trying to do. We aim to respond within 5 working days.

If you're not satisfied with our response, say so plainly and tell us again — we
would rather hear it twice than have you give up. Sovatela is published by one
person, so there is no second-line process to escalate to internally.

## Legal note

The European Accessibility Act applies to certain consumer software from 28 June
2025. Whether it reaches this product is genuinely unsettled: Sovatela is
published by an individual, free of charge, with no commercial activity — see
the legal and compliance checklist.

We have deliberately **not** named a national enforcement body to escalate to,
because we are not confident which Danish authority would be the right one for a
product of this kind, and sending someone to the wrong regulator wastes their
time rather than helping. If applicability is confirmed, the correct body will
be named here.

None of that changes the substance: this statement is written to be useful, and
the defects listed above are treated as defects, regardless of whether anyone is
legally required to fix them.
