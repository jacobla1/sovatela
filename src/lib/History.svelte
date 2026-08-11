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
</script>

<aside class="history">
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
            📁 <span class="proj-item-name">{p.name}</span>
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
    {#each groups as [label, items] (label)}
      <div class="history-group-head">{label}</div>
      {#each items as c (c.id)}
        <div class="history-item {c.id === currentId ? 'active' : ''}">
          <button class="history-open" onclick={() => onSelect?.(c.id)} title={c.title}>
            <span class="history-title">
              {#if runningIds[c.id]}<span class="run-dot" title="Working…" aria-label="Working"></span>
              {:else if doneIds[c.id]}<span class="done-dot" title="Finished" aria-label="Finished">●</span>{/if}
              {c.title || "Untitled"}
            </span>
          </button>
          <button
            class="history-del"
            title="Delete"
            aria-label="Delete conversation"
            onclick={() => onDelete?.(c.id)}
          >×</button>
        </div>
      {/each}
    {/each}
  </div>
</aside>
