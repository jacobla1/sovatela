<script>
  // Shared feature guide, shown in the first-run Overview and linked from Settings.

  import Icon from "./Icon.svelte";

  const features = [
    ["message", "Chat", "Talk to GLM-5.2 about anything."],
    ["paperclip", "Upload files & images", "Attach documents (PDF, Word, ODT, Excel, PowerPoint), text, code, or images and ask about them."],
    ["panel", "Create artifacts", "Ask for a chart, diagram, or small app — it renders in a panel on the right."],
    ["file", "Write documents", "Ask for a Word document, spreadsheet or slide deck — see what it will contain, then save it or have it written into your workspace folder."],
    ["image", "Generate images", "Turn on the image button in the message bar to create an image from a prompt — with FLUX, you can attach a picture for it to work from."],
    ["globe", "Search the web", "Turn on the web-search button for current, cited information."],
    ["bookmark", "Memory", "Tell it about you once, and it remembers useful facts across chats (you approve each)."],
    ["folder", "Projects", "Group chats with their own instructions and reference files (the ☰ sidebar)."],
    ["drive", "Workspace", "Give it a folder it can read and write — 'summarise these files' or 'research this and save a report.md'. It asks before every write and never deletes. (Settings → Workspace)"],
    ["chart", "Usage & cost", "See an indicative running cost, split by provider and kept on your device. (Settings → Usage & cost)"],
  ];

  const models = [
    ["Chat", "GLM-5.2 (Z.ai)"],
    ["Image understanding", "Mistral Small (Mistral)"],
    ["Image generation", "SDXL (Stability AI) or FLUX (Black Forest Labs)"],
  ];
</script>

<div class="guide">
  <p class="guide-intro">
    Chat with <strong>GLM-5.2</strong> — Z.ai's Chinese open model — hosted in
    <strong>France by Scaleway</strong>. This chat core is the sovereign part:
    your messages and image understanding run on Scaleway's EU infrastructure,
    under EU jurisdiction.
  </p>
  <div class="guide-list">
    {#each features as [icon, title, body]}
      <div class="guide-item">
        <span class="guide-icon"><Icon name={icon} size={20} /></span>
        <div>
          <strong>{title}</strong>
          <p>{body}</p>
        </div>
      </div>
    {/each}
  </div>

  <h3 class="guide-sub">Models</h3>
  <ul class="guide-facts">
    {#each models as [k, v]}
      <li><strong>{k}:</strong> {v}</li>
    {/each}
  </ul>

  <h3 class="guide-sub">What you'll need</h3>
  <ul class="guide-facts">
    <li>
      <strong>A Scaleway account + API key — the only requirement.</strong>
      This is what gives you the AI itself: chat, file &amp; image
      understanding, and artifacts all run on it. Connect it once and the app
      works. <em>(Settings → Scaleway API key)</em>
    </li>
  </ul>

  <h3 class="guide-sub">Optional add-ons</h3>
  <ul class="guide-facts">
    <li>
      <strong><Icon name="image" inline /> Image generation</strong> — connect a provider to create images
      from a prompt. <strong>OVHcloud</strong> (SDXL) is the recommended sovereign
      option — French and EU-hosted; <strong>Black Forest Labs</strong> (FLUX)
      gives the best quality but is a German-American company (US entity, and
      trains on your inputs by default); or point at your own self-hosted endpoint.
      Until then the image button stays off; everything else works without it.
      <em>(Settings → Image generation)</em>
    </li>
    <li>
      <strong><Icon name="globe" inline /> Web search</strong> — the AI has a fixed training cutoff, so on
      its own it can't know recent events, prices, or anything live from the web
      (and can be shaky on anything close to that cutoff). Connect a search
      provider and the web-search button lets it answer from current results:
      sign up for a free <strong>Linkup</strong> API key (easiest), use a
      search-server address from someone you trust, run a free local search proxy
      (needs Docker), or use a <strong>Qwant Staan</strong> key (business
      accounts). They differ on sovereignty: a local proxy and Qwant Staan are
      strongest, while Linkup keeps data in the EU but runs on Microsoft Azure
      (a US company) — Settings explains each and
      links their docs. <em>(Settings → Web search)</em>
    </li>
  </ul>

  <h3 class="guide-sub">Document templates</h3>
  <ul class="guide-facts">
    <li>
      <strong>Use a document you already have.</strong> Point at any Word
      document or presentation and everything generated comes out in its
      design — its fonts, colours, headings, page size, and any header or
      footer. <strong>Its text, its slides and its pictures stay behind</strong>,
      so last quarter's report works exactly as it is: there is nothing to
      empty out first. With none set, a plain built-in design is used.
      <em>(Settings → Document templates)</em>
    </li>
    <li>
      <strong>Set it in Settings, not by attaching it.</strong> Attaching a file
      to a message reads its <em>text</em>, which is the opposite of what you
      want from a template — so "make it look like this one" needs the setting.
      The header and footer are the one part of a template that does carry its
      wording: they are page furniture, so a letterhead reading
      <em>Q3 2025 — Confidential</em> will appear on what you generate.
    </li>
    <li>
      <strong>Some templates are refused, and the message says which one and
      why.</strong> Macros, a link to something outside the file, or a field
      that fetches when the document is opened: each of those would otherwise
      travel to whoever you send the document to, and fire on their machine
      rather than yours. Opening the file in Word and saving it again as a
      plain .docx or .pptx clears most of them.
    </li>
  </ul>
</div>
