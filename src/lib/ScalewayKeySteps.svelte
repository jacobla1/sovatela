<script>
  // The two Scaleway-side setup steps — open an account, generate a key —
  // shared by the Quick start page and by the Settings key form, which used to
  // carry near-identical copy each. They are the steps that describe someone
  // else's console, so they are the ones that go stale when Scaleway moves a
  // button; keeping them in one file means that is one edit, not two.
  //
  // The third step is deliberately NOT here. Each surface ends differently:
  // Quick start sends you to Settings, Settings puts the key field inline.
  //
  // Renders bare <li> elements so the caller supplies the <ol class="setup-steps">
  // and its own final step. The numerals are CSS counters on .setup-step, so
  // they keep counting across the seam without anything being passed in.
  import { invoke } from "@tauri-apps/api/core";

  // Rust vets the URL — scheme, credentials and length — before the
  // operating system sees it. See `open_external` in src-tauri/src/lib.rs.
  async function open(url) {
    try {
      await invoke("open_external", { url });
    } catch (e) {
      console.error("Could not open URL:", e);
    }
  }
</script>

<li class="setup-step">
  <div>
    <h2>Create a Scaleway account</h2>
    <p>
      Free to open. You add a card and pay only for what you use — ordinary chat
      runs to a few cents a day, and the first million tokens are free.
    </p>
    <button class="link" onclick={() => open("https://console.scaleway.com/register")}>
      Open Scaleway sign-up →
    </button>
  </div>
</li>

<li class="setup-step">
  <div>
    <h2>Create a Scaleway API key</h2>
    <p>
      Leave the settings as they are, pick <strong>No, skip for now</strong> for
      Object Storage, and copy the <strong>Secret key</strong> — Scaleway shows
      it only once, so do it before closing that window.
    </p>
    <p>
      Missed it? Generate another. Old keys keep working until you delete them,
      so a spare costs nothing.
    </p>

    <!-- Behind a disclosure on purpose. Most people need the four lines above;
         the ones who don't are stuck on a specific screen, and a ten-step wall
         shown to everyone makes setup look harder than it is. -->
    <details class="walkthrough">
      <summary>Show me exactly what to click</summary>
      <ol class="walkthrough-steps">
        <li>
          In the Scaleway console, open the drop-down menu at the
          <strong>top right</strong> and choose <strong>IAM &amp; API keys</strong>.
        </li>
        <li>Open the <strong>API keys</strong> tab, then <strong>+ Generate API key</strong>.</li>
        <li>
          <strong>Bearer</strong> — choose <strong>yourself</strong>. The other
          option, an IAM application, is for servers that run without a person
          attached; you do not need one.
        </li>
        <li>
          <strong>Description</strong> — optional. <em>Sovatela</em> makes it
          recognisable if you ever come back to a list of keys.
        </li>
        <li>
          <strong>Expiration</strong> — choose <strong>Never</strong>. Anything
          else means chat stops working on the day it lapses, and the error you
          get then will not mention the key.
        </li>
        <li>
          <strong>Object Storage</strong> — <strong>No, skip for now</strong>.
          It has nothing to do with chat.
        </li>
        <li>
          Click <strong>Generate API key</strong>. The screen that follows shows
          <strong>two</strong> values, and only one of them works here.
        </li>
      </ol>

      <!-- The likeliest setup failure by a distance: both values are on screen
           at once, and the wrong one is listed first. Scaleway's own wording for
           the difference is the clearest there is, so it is borrowed. -->
      <p class="walkthrough-note">
        <strong>Access key or Secret key?</strong> Scaleway describes the access
        key as “like a unique ID or username”, and the secret key as “like a
        password”. Sovatela needs the <strong>Secret key</strong>. Paste the
        wrong one and Scaleway simply rejects it.
      </p>
      <p class="walkthrough-note">
        A card on file gives you Scaleway's normal rate limits; verifying your
        identity with them raises those limits further. Neither is needed to
        generate the key.
      </p>
    </details>

    <button class="link" onclick={() => open("https://console.scaleway.com/iam/api-keys")}>
      Open API keys →
    </button>
  </div>
</li>
