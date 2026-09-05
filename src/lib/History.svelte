<script>
  // Left sidebar: projects + locally-saved conversations.
  let {
    conversations,
    currentId,
    onSelect,
    onNew,
    onDelete,
    projects = [],
    activeProjectId = null,
    onNewProject,
    onSelectProject,
    onExitProject,
    onEditProject,
    runningIds = {}, // cid → requestId for in-flight runs (spinner)
    doneIds = {}, // cid → true for background completions (badge)
    onSearch, // (query) => hits — reads the files in Rust
    onExport, // (id) => saves one conversation through a native dialog
    onRename, // (id, title) => the name as stored; throws with a reason
    onImport, // () => reads a chat back in through a native dialog
  } = $props();

  const activeProject = $derived(projects.find((p) => p.id === activeProjectId) || null);
  const knownIds = $derived(new Set(projects.map((p) => p.id)));

  // Inside a project: only its chats. Otherwise: chats not tied to any (existing) project.
  const shown = $derived(
    activeProjectId
      ? conversations.filter((c) => c.project_id === activeProjectId)
      : conversations.filter((c) => !c.project_id || !knownIds.has(c.project_id)),
  );

  // Bucket chats by recency (Today / Yesterday / …) so the list reads as a
  // browsable history instead of a flat log — and each row can drop its own
  // date, since the bucket header already conveys it. `shown` arrives sorted
  // newest-first, so items stay in order within each bucket.
  const DAY = 86400000;
  function startOfDay(v) {
    const d = new Date(v || 0);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  }
  const groups = $derived.by(() => {
    const today = startOfDay(Date.now());
    const buckets = [
      ["Today", []],
      ["Yesterday", []],
      ["Previous 7 days", []],
      ["Previous 30 days", []],
      ["Older", []],
    ];
    for (const c of shown) {
      const diff = today - startOfDay(c.updated_at);
      const i = diff <= 0 ? 0 : diff <= DAY ? 1 : diff < 7 * DAY ? 2 : diff < 30 * DAY ? 3 : 4;
      buckets[i][1].push(c);
    }
    return buckets.filter(([, items]) => items.length);
  });

  // Stable ids for the date headings, so each list can point at its own.
  const slug = (label) => label.toLowerCase().replace(/[^a-z0-9]+/g, "-");

  // ---------- Search ----------
  //
  // Searching reads the conversation files, so it is debounced rather than run
  // on every keystroke: typing "denmark" would otherwise be seven passes over
  // the folder to show the result of the seventh.
  //
  // Results replace the grouped list rather than filtering it. The groups are
  // date buckets, and a date bucket containing three matches from different
  // months is a worse answer than a flat list ordered by recency.
  let query = $state("");
  let results = $state(null); // null = not searching; [] = searched, nothing found
  let searching = $state(false);
  let searchError = $state("");
  let searchToken = 0;

  async function runSearch(q) {
    const mine = ++searchToken;
    if (!q.trim()) {
      results = null;
      searching = false;
      searchError = "";
      return;
    }
    searching = true;
    searchError = "";
    try {
      const hits = await onSearch?.(q);
      // A slower earlier search must not overwrite a later one's results.
      if (mine !== searchToken) return;
      results = hits ?? [];
    } catch (e) {
      if (mine !== searchToken) return;
      results = [];
      searchError = String(e?.message ?? e);
    } finally {
      if (mine === searchToken) searching = false;
    }
  }

  let debounce;
  function onQueryInput(v) {
    query = v;
    clearTimeout(debounce);
    debounce = setTimeout(() => runSearch(v), 200);
  }

  function clearSearch() {
    clearTimeout(debounce);
    query = "";
    results = null;
    searching = false;
    searchError = "";
  }

  // ---------- Renaming ----------
  //
  // Editing happens in the row rather than in a dialog: the name is being
  // chosen to tell this chat apart from the ones above and below it, and a
  // dialog covers exactly the list that comparison needs.
  //
  // Two ways in, because each one leaves somebody out. Double-clicking the row
  // is the idiom people already have from every file manager, and no keyboard
  // has it. F2 is the keyboard's rename key, and it is invisible — so it is in
  // the shortcuts sheet, which is the only reason it counts as reachable.
  let renamingId = $state(null);
  let draft = $state("");
  let renameError = $state("");
  // Enter removes the input, which blurs it, which would commit a second time.
  let committing = false;

  function startRename(c) {
    renamingId = c.id;
    draft = c.title || "";
    renameError = "";
  }

  function cancelRename() {
    renamingId = null;
    draft = "";
    renameError = "";
  }

  async function commitRename(id, original) {
    if (committing) return;
    // An empty box or an unchanged name leaves quietly rather than reporting
    // anything: clearing the field and pressing Enter reads as "never mind",
    // and rewriting a file to store the name already in it is not a rename.
    // The backend still refuses an empty name — a command has to defend itself
    // whatever its caller does.
    const next = draft.trim();
    if (!next || next === (original || "").trim()) {
      cancelRename();
      return;
    }
    committing = true;
    try {
      await onRename?.(id, next);
      cancelRename();
    } catch (e) {
      // Stay in the editing state and say why. Closing the box on a failure
      // would leave the old name in place with nothing to distinguish that
      // from a rename that worked and had not yet redrawn.
      renameError = String(e?.message ?? e);
    } finally {
      committing = false;
    }
  }

  // Focus without stealing it back afterwards: this runs once, when the input
  // replaces the button, so clicking away from a failed rename still works.
  function takeFocus(node) {
    node.focus();
    node.select();
  }
</script>

<!-- The row's title, editable in place. One definition, used by both the
     grouped list and the search results, so a chat cannot be renameable in one
     and not the other depending on whether you found it by scrolling. -->
{#snippet titleCell(c, body)}
  {#if renamingId === c.id}
    <span class="history-renaming">
      <input
        class="history-rename"
        type="text"
        value={draft}
        oninput={(e) => (draft = e.currentTarget.value)}
        aria-label={`Rename conversation: ${c.title || "Untitled"}`}
        aria-invalid={renameError ? "true" : undefined}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commitRename(c.id, c.title);
          } else if (e.key === "Escape") {
            // Stops here: Escape further up clears the search box, and
            // abandoning a rename should not also throw away the search that
            // found the chat being renamed.
            e.stopPropagation();
            cancelRename();
          }
        }}
        onblur={() => commitRename(c.id, c.title)}
        autocomplete="off"
        spellcheck="false"
        use:takeFocus
      />
      {#if renameError}
        <span class="history-rename-error" role="alert">{renameError}</span>
      {/if}
    </span>
  {:else}
    <button
      class="history-open"
      onclick={() => onSelect?.(c.id)}
      ondblclick={() => startRename(c)}
      onkeydown={(e) => e.key === "F2" && (e.preventDefault(), startRename(c))}
      title={c.title}
      aria-current={c.id === currentId ? "true" : undefined}
    >
      {@render body()}
    </button>
  {/if}
{/snippet}

<!-- nav rather than aside: this is how someone moves between conversations,
     which is navigation. Named, because a landmark without a name is announced
     only as "navigation". -->
<nav class="history" aria-label="Chats and projects">
  <button class="new-chat" onclick={() => onNew?.()}>
    <span class="new-chat-icon" aria-hidden="true">+</span>
    <span>{activeProject ? `New chat in ${activeProject.name}` : "New chat"}</span>
  </button>

  <!-- Searching reads every saved conversation, so it is a deliberate act: a
       field you type into, not a filter that runs as the list renders. -->
  <div class="history-search">
    <label class="sr-only" for="history-search-input">Search saved chats</label>
    <input
      id="history-search-input"
      type="search"
      placeholder="Search chats…"
      value={query}
      oninput={(e) => onQueryInput(e.currentTarget.value)}
      onkeydown={(e) => e.key === "Escape" && clearSearch()}
      autocomplete="off"
      spellcheck="false"
    />
    {#if query}
      <button class="history-search-clear" aria-label="Clear search" onclick={clearSearch}>×</button>
    {/if}
  </div>

  {#if activeProject}
    <div class="proj-header">
      <button class="proj-back" onclick={() => onExitProject?.()}>← All chats</button>
      <div class="proj-current">
        <span class="proj-name" title={activeProject.name}>📁 {activeProject.name}</span>
        <button
          class="proj-edit"
          title="Edit project"
          aria-label="Edit project"
          onclick={() => onEditProject?.(activeProject.id)}
        >⚙</button>
      </div>
    </div>
  {:else}
    <div class="proj-section">
      <div class="proj-section-head">
        <span>Projects</span>
        <button class="proj-add" title="New project" aria-label="New project"
          onclick={() => onNewProject?.()}>+</button>
      </div>
      {#if projects.length === 0}
        <p class="history-empty muted">No projects yet.</p>
      {:else}
        {#each projects as p (p.id)}
          <button class="proj-item" onclick={() => onSelectProject?.(p.id)} title={p.name}>
            <span aria-hidden="true">📁</span> <span class="proj-item-name">{p.name}</span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}

  {#if !activeProject}
    <!-- Import sits on the list's own heading rather than in Settings: a chat
         file is a chat, not a preference, and this is where someone who
         exported one comes looking for it. ↑ against the rows' ↓, since the two
         are the same operation in opposite directions. -->
    <div class="history-head">
      <span>Recent chats</span>
      <button
        class="history-import"
        title="Import a chat from a file"
        aria-label="Import a chat from a file"
        onclick={() => onImport?.()}
      >↑</button>
    </div>
  {/if}

  <div class="history-list">
    {#if results !== null}
      <!-- Searching: a flat list, newest first. The date groups are a way of
           browsing; when you have asked a question, the answer is the answer. -->
      <div class="history-group-head" id="hist-group-results" aria-live="polite">
        {#if searching}
          Searching…
        {:else if searchError}
          Search failed
        {:else}
          {results.length === 0
            ? "No chats match"
            : `${results.length} chat${results.length === 1 ? "" : "s"} match`}
        {/if}
      </div>
      {#if searchError}
        <p class="warn-text" role="alert">{searchError}</p>
      {/if}
      <ul class="history-group" role="list" aria-labelledby="hist-group-results">
        {#each results as c (c.id)}
          <li role="listitem" class="history-item result {c.id === currentId ? 'active' : ''}">
            {#snippet resultBody()}
              <span class="history-title">{c.title || "Untitled"}</span>
              {#if c.snippet_match}
                <!-- The match is marked rather than left for the reader to
                     find. Three fields rather than one string of markup: this
                     is the user's own text, and a conversation may contain
                     angle brackets. -->
                <span class="history-snippet"
                  >{c.snippet_before}<mark>{c.snippet_match}</mark>{c.snippet_after}</span
                >
              {:else if c.title_matched}
                <span class="history-snippet muted">matches the title</span>
              {/if}
            {/snippet}
            {@render titleCell(c, resultBody)}
            <button
              class="history-export"
              title="Export this chat"
              aria-label={`Export conversation: ${c.title || "Untitled"}`}
              onclick={() => onExport?.(c.id)}
            >↓</button>
          </li>
        {/each}
      </ul>
    {:else}
    {#if shown.length === 0}
      <p class="history-empty muted">
        {activeProject ? "No chats in this project yet." : "No saved chats yet."}
      </p>
    {/if}
    <!-- Real lists, grouped under their date heading. As a pile of divs a
         screen reader announced a run of buttons with no sense of how many
         chats there were or which one this was; a list says "3 of 12". The
         heading is referenced rather than repeated into every row. -->
    {#each groups as [label, items] (label)}
      <div class="history-group-head" id="hist-group-{slug(label)}">{label}</div>
      <!-- role="list" restores what list-style:none takes away in WebKit,
           which is the engine this app runs on: VoiceOver otherwise stops
           announcing an unstyled list as a list at all.

           role="listitem" is the other half, and it was missing — which is the
           chat-list failure the accessibility statement records. WebKit drops
           the implicit listitem role from an <li> that is display:flex, and
           .history-item is, because a row is a title beside a delete button.
           So the container announced as a list while its rows announced as
           nothing: no "3 of 12", no sense of how many chats there were or which
           one this was, which is the entire reason for marking it up as a list.
           Restoring the role on the <ul> alone cannot fix that; the roles are
           dropped by different rules and have to be restored separately. -->
      <ul class="history-group" role="list" aria-labelledby="hist-group-{slug(label)}">
        {#each items as c (c.id)}
          <li role="listitem" class="history-item {c.id === currentId ? 'active' : ''}">
            {#snippet rowBody()}
              <span class="history-title">
                {#if runningIds[c.id]}<span class="run-dot" title="Working…" aria-label="Working"></span>
                {:else if doneIds[c.id]}<span class="done-dot" title="Finished" aria-label="Finished">●</span>{/if}
                {c.title || "Untitled"}
              </span>
            {/snippet}
            {@render titleCell(c, rowBody)}
            <button
              class="history-export"
              title="Export this chat"
              aria-label={`Export conversation: ${c.title || "Untitled"}`}
              onclick={() => onExport?.(c.id)}
            >↓</button>
            <button
              class="history-del"
              title="Delete"
              aria-label={`Delete conversation: ${c.title || "Untitled"}`}
              onclick={() => onDelete?.(c.id)}
            >×</button>
          </li>
        {/each}
      </ul>
    {/each}
    {/if}
  </div>
</nav>
