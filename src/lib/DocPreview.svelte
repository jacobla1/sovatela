<script>
  // What the file will contain, drawn from the writer's own answer.
  //
  // Nothing here parses Markdown. The blocks, the slides and the rows all
  // arrive from `preview_document`, which is the same code that writes the
  // file — so this component's whole job is to draw what it is given, and it
  // has no opinion about what a construct means. That is the point: the old
  // preview ran `marked` over the same source and disagreed with the file on
  // sixteen of twenty-four constructs, none of which was visible until
  // somebody opened the document.
  let { preview } = $props();

  // Word has three heading styles. Which one a heading gets depends on the
  // template, and a template that defines none leaves the paragraph as body
  // text — so the preview reads the style rather than the depth. `null` means
  // Word will render it as an ordinary paragraph, and so do we.
  const headingTag = (style) =>
    ({ Heading1: "h1", Heading2: "h2", Heading3: "h3" })[style] ?? "p";
</script>

{#snippet runs(spans)}
  {#each spans as s}
    {#if s.t === "bold"}<strong>{s.v}</strong>
    {:else if s.t === "italic"}<em>{s.v}</em>
    {:else if s.t === "code"}<code>{s.v}</code>
    {:else}{s.v}{/if}
  {/each}
{/snippet}

{#if preview?.type === "blocks"}
  <div class="doc">
    {#each preview.blocks as b}
      {#if b.kind === "heading"}
        {@const tag = headingTag(b.style)}
        {#if tag === "h1"}<h1>{@render runs(b.spans)}</h1>
        {:else if tag === "h2"}<h2>{@render runs(b.spans)}</h2>
        {:else if tag === "h3"}<h3>{@render runs(b.spans)}</h3>
        {:else}
          <!-- The template defines no style for this depth, so Word will draw
               it as body text. Showing a heading here would promise one. -->
          <p>{@render runs(b.spans)}</p>
        {/if}
      {:else if b.kind === "para"}
        <p>{@render runs(b.spans)}</p>
      {:else if b.kind === "item"}
        <!-- An indented paragraph carrying a literal marker, which is what the
             writer emits. A real <ul> would imply a list the file does not
             have — the items are not machine-numbered. -->
        <p class="item"><span class="marker">{b.marker}</span>{@render runs(b.spans)}</p>
      {:else if b.kind === "table"}
        <div class="scroll">
          <table>
            <thead>
              <tr>{#each b.rows[0] ?? [] as cell}<th>{@render runs(cell)}</th>{/each}</tr>
            </thead>
            <tbody>
              {#each b.rows.slice(1) as row}
                <tr>{#each row as cell}<td>{@render runs(cell)}</td>{/each}</tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else if b.kind === "rule"}
        <hr />
      {/if}
    {/each}
  </div>
{:else if preview?.type === "slides"}
  <!-- Slides, not the source's blocks: which headings divide the deck, where
       an overlong slide is split and which titles gain "(cont.)" are all
       decided by the writer. -->
  <ol class="slides">
    {#each preview.slides as slide, i}
      <li class="slide">
        <span class="slide-number">{i + 1}</span>
        <div class="slide-body">
          <p class="slide-title">{slide.title}</p>
          {#each slide.bullets as bullet}
            <p class="item"><span class="marker">•</span>{bullet}</p>
          {/each}
        </div>
      </li>
    {/each}
  </ol>
{:else if preview?.type === "table"}
  <div class="doc">
    {#if preview.rows.length}
      <div class="scroll">
        <table>
          <thead>
            <tr>{#each preview.rows[0] as cell}<th>{cell}</th>{/each}</tr>
          </thead>
          <tbody>
            {#each preview.rows.slice(1) as row}
              <tr>{#each row as cell}<td>{cell}</td>{/each}</tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
{/if}

<style>
  /* A document preview reads as a document, not as chat: the page's own type,
     visible table rules, figures aligned. It shows what the file will say, not
     what Word will make of it — imitating Word would promise a fidelity the
     conversion does not have. */
  .doc {
    padding: var(--sp-3, 1rem);
    line-height: 1.6;
  }
  h1,
  h2,
  h3 {
    margin: 1.2em 0 0.4em;
    line-height: 1.25;
  }
  h1 { font-size: 1.4rem; }
  h2 { font-size: 1.2rem; }
  h3 { font-size: 1.05rem; }
  p { margin: 0 0 0.8em; }
  .item {
    margin: 0 0 0.25em;
    padding-left: 1.6em;
    text-indent: -1.6em;
  }
  .marker {
    display: inline-block;
    min-width: 1.6em;
    text-indent: 0;
  }
  hr {
    border: 0;
    border-top: 1px solid var(--border-control, #ccc);
    margin: 1.2em 0;
  }
  /* A table wider than the panel scrolls inside its own box rather than
     pushing the conversation sideways. */
  .scroll {
    overflow-x: auto;
    margin: 0 0 0.8em;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 0.92em;
  }
  th,
  td {
    border: 1px solid var(--border-control, #ccc);
    padding: 0.35em 0.55em;
    text-align: left;
  }
  th { font-weight: 600; }
  /* Figures line up when they are meant to be compared. */
  td { font-variant-numeric: tabular-nums; }

  .slides {
    list-style: none;
    margin: 0;
    padding: var(--sp-3, 1rem);
    display: flex;
    flex-direction: column;
    gap: var(--sp-3, 1rem);
  }
  .slide {
    display: flex;
    gap: 0.75rem;
    align-items: flex-start;
  }
  .slide-number {
    flex: none;
    min-width: 1.6rem;
    padding-top: 0.15rem;
    color: var(--muted, #5b6570);
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .slide-body {
    flex: 1;
    min-width: 0;
    border: 1px solid var(--border-control, #ccc);
    border-radius: 4px;
    padding: 0.7rem 0.9rem;
    /* Sized to its content, not to a slide's proportions. A 16:9 box looked
       right for one slide and was wrong for a deck: a title card with a single
       line sat in ten times its own height of nothing, and a ten-slide deck
       took ten screens to scroll past. It could not earn that back by showing
       whether the content fits, either — the type here is not the type on the
       slide, and the question is already answered by how many cards there are,
       since the writer splits a slide that overflows. */
    min-height: 4.5rem;
  }
  .slide-title {
    margin: 0 0 0.5em;
    font-weight: 600;
    line-height: 1.25;
  }
  .slide-body .item {
    font-size: 0.92em;
  }
</style>
