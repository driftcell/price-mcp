# PriceBook MCP

Product and price catalogue exposed as an MCP server on **Cloudflare Workers +
D1**. A quoting Agent reads customer design drawings, retrieves products, prices
and pricing context, then prepares the quote itself. The application does not
implement an automatic pricing engine.

> **Experimental runtime:** this version uses Cloudflare's
> `wasm32-unknown-emscripten` preview and its Tokio patchset. It is intended as a
> deployment experiment, not a stable production baseline.

## Architecture

- Rust Worker entrypoint with Axum.
- Streamable HTTP MCP at `/mcp`; stdio is no longer supported.
- Stateless MCP operation with JSON responses, no in-memory MCP sessions.
- Cloudflare D1 through a Workers binding, with JSON stored as TEXT.
- Bearer authentication for `/mcp`; `/health` is public.
- All existing read and write MCP tools are retained.

## Prerequisites

- `cf` CLI, authenticated with `cf auth login`.
- Node.js/npm (Wrangler is used for local D1 imports).
- Rustup. `rust-toolchain.toml` selects the required beta toolchain and
  Emscripten target.
- Python 3.10 or newer on `PATH` while building Emscripten (macOS's system
  Python 3.9 is too old). With uv, use `uv python install 3.12` and prepend the
  directory from `uv python find 3.12` to `PATH`.

Install the JavaScript development dependency:

```bash
npm install
```

## D1 setup

Create a database for another environment, or use the database already bound in
`cloudflare.config.ts` for this experiment:

```bash
cf d1 create --name pricebook-mcp-experiment --primary-location-hint apac
```

Put the returned UUID in `cloudflare.config.ts` (and in the local-only
`wrangler.toml`), then apply the remote schema:

```bash
cf d1 migrations apply <database-id> --dir migrations
```

D1 migrations are applied through Wrangler/Cloudflare's migration bookkeeping,
not when a Worker request starts. This experiment edits `0001` for a **fresh D1
database**. A database that already applied the old JSONB schema must be rebuilt
or given a separate data-conversion migration; changing `0001` alone does not
upgrade it.

## Migrate the existing SQLite data

`tools/export_d1.py` reads `data/pricebook.db`, converts all SQLite JSONB columns
to JSON TEXT, and writes bounded INSERT statements to `data/pricebook.d1.sql`:

```bash
python3 tools/export_d1.py
```

For Wrangler's local D1 simulator, create the schema and import the generated
file. These commands use `.wrangler/state`, separate from `cf --local` state:

```bash
npx wrangler d1 execute pricebook-mcp-experiment \
  --local --file=migrations/0001_initial_schema.sql
npx wrangler d1 execute pricebook-mcp-experiment \
  --local --file=data/pricebook.d1.sql
```

For a new, empty remote D1 database, use the `cf`-based batch importer:

```bash
python3 tools/import_d1.py <database-id>
```

The remote importer is intentionally not idempotent. Run it only once against an
empty schema. It does not implement `cf d1 query --local`, because that CLI
operation has no local equivalent.

## Local development

Create `.dev.vars` (ignored by Git):

```dotenv
MCP_BEARER_TOKEN=local-test-token
```

Then run:

```bash
cf dev --local
```

Endpoints:

```text
http://127.0.0.1:8787/health
http://127.0.0.1:8787/mcp
```

Send `Authorization: Bearer local-test-token` for MCP requests. The
`MCP_ALLOWED_HOSTS` variable in `cloudflare.config.ts` is a comma-separated Host
allowlist; include the public hostname before testing a deployment.

Run the read-only smoke test, or include temporary create/delete and rollback
checks (the same command works against a deployed URL):

```bash
python3 tools/smoke_mcp.py
python3 tools/smoke_mcp.py --write
python3 tools/smoke_mcp.py --base-url https://<your-worker>.workers.dev \
  --token '<random-token>' --write
```

## Deploy

First perform a build-only check:

```bash
cf deploy --dry-run
```

Create an ignored `.deploy.vars` containing a random token, then deploy it with
the Worker version:

```dotenv
MCP_BEARER_TOKEN=<random-token>
```

```bash
cf deploy --secrets-file .deploy.vars
```

The MCP endpoint rejects requests when this secret is absent. Add the deployed
`*.workers.dev` hostname to `MCP_ALLOWED_HOSTS` and redeploy before connecting a
client.

## MCP tools

The Agent workflow is intentionally two-stage:

1. **`search_products`** — submit all decoded drawing items in one batch.
2. **`search_colors`** — resolve color names/codes to classes and aliases used by
   price conditions.
3. **`get_quote_data`** — fetch selected products, matching variants, all
   factory/retail price rows and conditions, relations, pricing rules and
   documents.

`list_price_books` exposes the active series/domain/version scope. The Agent,
not the MCP server, selects prices and calculates the quote.

Editing tools remain available: `save_products`, `save_variants`,
`save_product_codes`, `save_prices`, `save_relations`, `save_pricing_rules`,
`save_documents`, `delete_products` and `delete_records`.

## D1 differences and limitations

- JSON fields are D1 TEXT values constrained by `json_valid()`. The original
  local import helpers still use SQLite JSONB; export converts them.
- Money remains exact decimal text in `prices.amount_decimal`.
- D1 has no SQLx connection pool, WAL configuration or runtime migrations.
- Each write tool validates/reads first, then commits queued writes in one atomic
  D1 `batch()`. These reads are **not** in the same interactive transaction and
  do not see queued writes. Concurrent read/modify/write operations therefore
  have weaker isolation than the original SQLite version. Repeated updates of
  the same ID in one call are rejected to avoid stale full-row overwrites.
- Product creation allocates explicit IDs before the batch because D1 does not
  reliably propagate `last_insert_rowid()` between batched statements. Products
  and scopes still commit atomically. A concurrent ID allocation conflict fails
  the batch and can be retried; it must not be silently ignored.
- MCP runs without legacy sessions or resumable SSE. Server-initiated
  notifications and long-lived background tasks are outside this experiment.
- D1 bind parameters are limited to 100 per query, and JavaScript-facing integer
  values must remain within the safe 53-bit range. Current catalogue IDs are
  small enough.

## OpenCode connection

```jsonc
{
  "mcp": {
    "servers": {
      "pricebook": {
        "type": "remote",
        "url": "https://<your-worker>.workers.dev/mcp",
        "headers": {
          "Authorization": "Bearer <random-token>"
        },
        "oauth": false
      }
    }
  }
}
```

## Offline import tooling

`tools/pricebook_db.py` remains the low-level helper for human/Agent-reviewed
imports into the original local SQLite staging database. After reviewing and
validating those imports, regenerate the D1 SQL with `tools/export_d1.py`.
