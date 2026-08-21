# Sovereign image generation (LiteLLM → FLUX EU)

Enables the app's 🎨 image generation using **FLUX by Black Forest Labs** (a
German model) through its **EU endpoint** — no GPU required. A small
[LiteLLM](https://litellm.ai) proxy translates the app's OpenAI-images request
into BFL's API and returns the generated image.

```
app  →  LiteLLM proxy (/v1/images/generations)  →  api.eu.bfl.ai (FLUX)
```

You bring your **own BFL API key** and pay per image — nothing is shared.

## Prerequisites

- **Docker Desktop** (free).
- A **Black Forest Labs API key** with credits — <https://bfl.ai>.

## Run it

```sh
cd deploy/flux-litellm
cp .env.example .env
#   edit .env:
#     BFL_API_KEY        = your BFL key
#     LITELLM_MASTER_KEY = sk- + random  (the sk- prefix is REQUIRED by LiteLLM;
#                          without it you get "No connected db"):
#                          echo "sk-$(openssl rand -hex 32)"
docker compose up -d

# smoke test — returns JSON with "data":[{"url":"https://..."}]:
curl -s http://localhost:4000/v1/images/generations \
  -H "Authorization: Bearer $(grep LITELLM_MASTER_KEY .env | cut -d= -f2)" \
  -H "Content-Type: application/json" \
  -d '{"model":"flux","prompt":"a red bicycle on a beach"}' \
  | head -c 200
```

## Point the app at it

**Settings → Image generation:**
- **Image endpoint URL:** `http://localhost:4000/v1/images/generations`
- **Model:** `flux`
- **Access token:** your `LITELLM_MASTER_KEY`

Save, then toggle 🎨 in chat and describe an image.

## Notes

- **Master key needs `sk-`:** LiteLLM only accepts a master key that starts with
  `sk-`. Without it, every request falls through to a database lookup and fails
  with `"No connected db"`. Use the same `sk-…` value in the app's Access token.
- **Returns a URL, not base64:** BFL rejects the `response_format` param, so
  `config.yaml` sets `drop_params: true`; BFL then returns an image **URL**
  (`data[0].url`), which the app loads from `api.eu.bfl.ai` when displaying.
- **Sovereign:** requests go to `api.eu.bfl.ai` (EU) for a German model. Verify
  the endpoint/model id in `config.yaml` against the current
  [LiteLLM](https://docs.litellm.ai/docs/providers/black_forest_labs) and
  [BFL](https://docs.bfl.ai/) docs — they may rename models over time.
- **Cost:** BFL bills per image to your key. `flux-pro-1.1` is higher quality;
  switch `config.yaml` to `flux-dev` for cheaper generations.
- **Remote use:** to reach it from another machine, front it with nginx + TLS
  and use that URL instead of `localhost`.
