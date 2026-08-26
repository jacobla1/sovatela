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
</script>

<!-- nav rather than aside: this is how someone moves between conversations,
     which is navigation. Named, because a landmark without a name is announced
     only as "navigation". -->
<nav class="history" aria-label="Chats and projects">
  <button class="new-chat" onclick={() => onNew?.()}>
    <span class="new-chat-icon" aria-hidden="true">+</span>
    <span>{activeProject ? `New chat in ${activeProject.name}` : "New chat"}</span>
  </button>

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
    <div class="history-head">Recent chats</div>
  {/if}

  <div class="history-list">
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
           announcing an unstyled list as a list at all. -->
      <ul class="history-group" role="list" aria-labelledby="hist-group-{slug(label)}">
        {#each items as c (c.id)}
          <li class="history-item {c.id === currentId ? 'active' : ''}">
            <button
              class="history-open"
              onclick={() => onSelect?.(c.id)}
              title={c.title}
              aria-current={c.id === currentId ? "true" : undefined}
            >
              <span class="history-title">
                {#if runningIds[c.id]}<span class="run-dot" title="Working…" aria-label="Working"></span>
                {:else if doneIds[c.id]}<span class="done-dot" title="Finished" aria-label="Finished">●</span>{/if}
                {c.title || "Untitled"}
              </span>
            </button>
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
  </div>
</nav>
