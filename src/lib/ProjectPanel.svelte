<script>
  // Modal for editing a project: name, instructions, and file knowledge.
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    MAX_TEXT_BYTES,
    MAX_DOC_BYTES,
    readAs,
    looksBinary,
    isExtractableDocument,
    isLegacyDocument,
    legacyDocumentHint,
    extractDocument,
  } from "./files.js";

  import { untrack, tick } from "svelte";

  let { project, onSave, onDelete, onClose } = $props();

  // Seeded from the project once, on purpose: these are the fields being
  // edited, so they must not be overwritten from the prop while someone is
  // typing. The parent renders this inside {#key editingProject.id}, so a
  // different project remounts the component rather than reusing this state.
  // `untrack` says that rather than leaving the compiler to guess — it warned
  // about the ambiguity, and the warning was fair even though the behaviour
  // was not wrong.
  let name = $state(untrack(() => project.name || ""));
  let instructions = $state(untrack(() => project.instructions || ""));
  let files = $state(untrack(() => [...(project.files || [])]));
  let error = $state("");
  let fileInput;

  // ----- Focus -----
  //
  // The dialog declared role="dialog" and aria-modal="true" and did none of
  // what those promise: focus stayed on whatever opened it, Tab walked out
  // into the page behind, and closing left focus nowhere. A screen reader was
  // told it had entered a dialog while the keyboard was still outside it.
  let modalEl = $state(null);
  let nameEl = $state(null);
  // Captured before the dialog takes focus, so it can be handed back.
  const opener = typeof document !== "undefined" ? document.activeElement : null;

  $effect(() => {
    if (!modalEl) return;
    // The name field is the first thing anyone came here to change.
    tick().then(() => (nameEl ?? modalEl)?.focus());
    return () => {
      // Return focus where it came from. `isConnected` because the trigger can
      // be gone by now — deleting a project removes the row that opened this.
      if (opener?.isConnected) opener.focus();
    };
  });

  /** Everything inside the dialog that can hold focus, in document order. */
  function focusables() {
    if (!modalEl) return [];
    return [
      ...modalEl.querySelectorAll(
        'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
      // Not `offsetParent !== null`, the usual shorthand for "is visible": it
      // is also null for anything position:fixed, which would silently drop a
      // real control out of the trap. Ask about the properties that actually
      // mean hidden.
    ].filter((el) => {
      if (el.hidden) return false;
      const style = getComputedStyle(el);
      return style.display !== "none" && style.visibility !== "hidden";
    });
  }

  function onKeydown(e) {
    if (e.key === "Escape") {
      onClose?.();
      return;
    }
    if (e.key !== "Tab") return;
    const items = focusables();
    if (items.length === 0) return;
    const first = items[0];
    const last = items[items.length - 1];
    // Wrap at both ends, and catch the case where focus has somehow left the
    // dialog already — otherwise Tab escapes into the page behind the modal.
    if (e.shiftKey && (document.activeElement === first || !modalEl.contains(document.activeElement))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  async function onFiles(list) {
    error = "";
    for (const f of Array.from(list)) {
      if (f.type.startsWith("image/")) {
        error = "Project files are text or documents for now — images aren't supported here yet.";
        continue;
      }
      try {
        if (isExtractableDocument(f)) {
          // PDF / .docx / .odt → the Rust backend extracts the real text.
          if (f.size > MAX_DOC_BYTES) {
            error = `${f.name} is too large (max 20 MB).`;
            continue;
          }
          const content = await extractDocument(f);
          files.push({ name: f.name, content });
        } else if (isLegacyDocument(f)) {
          error = legacyDocumentHint(f);
        } else {
          if (f.size > MAX_TEXT_BYTES) {
            error = `${f.name} is too large (max 400 KB).`;
            continue;
          }
          const content = await readAs(f, "text");
          // Reject binary garbage rather than feeding it into every project chat.
          if (looksBinary(content)) {
            error = `${f.name} isn't a readable text file.`;
            continue;
          }
          files.push({ name: f.name, content });
        }
      } catch (e) {
        error = `Could not read ${f.name}: ${e}`;
      }
    }
    if (fileInput) fileInput.value = ""; // allow re-selecting the same file
  }

  function removeFile(i) {
    files.splice(i, 1);
  }

  function save() {
    onSave?.({
      ...project,
      name: name.trim() || "Untitled project",
      instructions: instructions.trim(),
      files: files.map((f) => ({ name: f.name, content: f.content })),
      updated_at: new Date().toISOString(),
    });
  }

  async function confirmDelete() {
    // window.confirm is unreliable inside Tauri webviews — use the native dialog.
    const ok = await ask("Its chats are kept, but ungrouped.", {
      title: `Delete the project “${project.name}”?`,
      kind: "warning",
    });
    if (ok) onDelete?.(project.id);
  }
</script>

<!-- Close only when the backdrop itself is clicked. Comparing target to
     currentTarget removes the stopPropagation handler the dialog used to carry,
     which was a click listener on a non-interactive element with no keyboard
     equivalent — Escape is handled above, and now nothing else needs one. -->
<div
  class="modal-backdrop"
  onclick={(e) => e.target === e.currentTarget && onClose?.()}
  role="presentation"
>
  <!-- tabindex="-1" so focus can be moved into the dialog programmatically
       without adding it to the tab order, which is what role="dialog" needs.
       Escape and the Tab trap are handled here rather than on the window, so
       they belong to the dialog and stop existing when it does. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-labelledby="project-modal-title"
    bind:this={modalEl}
    onkeydown={onKeydown}
  >
    <div class="modal-head">
      <h2 id="project-modal-title">Project</h2>
      <button class="modal-close" aria-label="Close" onclick={() => onClose?.()}>×</button>
    </div>

    <div class="modal-body">
      <label class="field">
        <span>Name</span>
        <input type="text" bind:value={name} placeholder="e.g. Thesis, Client X, Home reno"
          autocomplete="off" spellcheck="false" bind:this={nameEl} />
      </label>

      <label class="field">
        <span>Project instructions</span>
        <textarea rows="4" bind:value={instructions} spellcheck="false"
          placeholder="How the assistant should behave in this project — e.g. “You're my thesis advisor. Cite sources. Reply in academic Danish.”"
        ></textarea>
      </label>

      <div class="field">
        <span class="field-label">Files</span>
        <p class="hint">
          Add documents (PDF, Word, ODT) or text files as reference material.
          Their contents are sent with every chat in this project.
        </p>
        {#if files.length}
          <ul class="proj-files">
            {#each files as f, i (i)}
              <li>
                <span class="proj-file-name" title={f.name}>{f.name}</span>
                <button class="proj-file-del" aria-label={`Remove file: ${f.name}`} onclick={() => removeFile(i)}>×</button>
              </li>
            {/each}
          </ul>
        {/if}
        <input
          class="file-input-hidden"
          type="file"
          multiple
          bind:this={fileInput}
          onchange={(e) => onFiles(e.currentTarget.files)}
        />
        <button class="ghost" onclick={() => fileInput?.click()}>+ Add files</button>
      </div>

      {#if error}<p class="error">{error}</p>{/if}
    </div>

    <div class="modal-foot">
      <button class="danger" onclick={confirmDelete}>Delete project</button>
      <div class="modal-foot-right">
        <button class="link" onclick={() => onClose?.()}>Cancel</button>
        <button class="primary" onclick={save}>Save</button>
      </div>
    </div>
  </div>
</div>
