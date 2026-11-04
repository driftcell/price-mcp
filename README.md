# PriceBook MCP

SQLite-backed product and price catalogue for an MCP server. A quoting Agent
reads customer design drawings, uses MCP tools to retrieve products, prices and
pricing context, then prepares the quote itself. The application does not
implement an automatic pricing engine.

## Current state

- Rust crate scaffolded with Tokio and SQLx 0.9.
- SQLite schema in `migrations/0001_initial_schema.sql`.
- Database created at `data/pricebook.db`.
- All five price books imported and active: J kitchen, P home, P kitchen,
  S home and S kitchen.
- Imported data: 1,210 products, 3,838 variants, 2,682 codes, 123 colors,
  10,493 price rows, 193 pricing rules, 1,677 product relations and 62 documents.
- Products, variants, aliases, colors, prices, pricing rules, product relations,
  documents and optional images are modelled.
- Catalogue images are intentionally deferred; the quoting Agent reads customer
  design drawings rather than catalogue images.

## Run

```bash
cargo run -- --db data/pricebook.db --host 127.0.0.1 --port 8787
```

The Streamable HTTP MCP endpoint is:

```text
http://127.0.0.1:8787/mcp
```

Health check:

```text
GET http://127.0.0.1:8787/health
```

### stdio

For MCP clients that launch the server as a child process:

```bash
cargo build --release
./target/release/pricebook-mcp --transport stdio --db /absolute/path/to/data/pricebook.db
```

The process speaks MCP over stdin/stdout. Logs go to stderr so they never
corrupt the protocol stream. Prefer an absolute `--db` path because the client
chooses the working directory.

The server opens the database and applies pending migrations at startup.

## MCP tools

The Agent workflow is intentionally two-stage:

1. **`search_products`** — submit all decoded drawing items in one batch.
   Returns candidate product IDs, categories, codes, statuses and sample
   variants.
2. **`search_colors`** — submit all color names/codes in one batch and resolve
   them to color classes and aliases used by price conditions.
3. **`get_quote_data`** — submit the chosen product IDs/codes in one batch,
   optionally with exact dimension/attribute filters. Returns matching variants,
   all factory/retail price rows and conditions, relations, pricing rules and
   documents.

`list_price_books` exposes the active series/domain/version scope.

`get_quote_data` does not select a price or calculate a quote. The Agent reads
the conditions and rules and prepares the quote itself. Missing prices,
ambiguous codes, discontinued items and truncated variant results are returned
as warnings or candidate lists rather than guessed.

## Editing tools

The server also exposes standard MCP write tools. They execute immediately; any
human-confirmation policy belongs in the client or in Agent skills.

- `save_products`
- `save_variants`
- `save_product_codes`
- `save_prices`
- `save_relations`
- `save_pricing_rules`
- `save_documents`
- `delete_products` (`retire` or `hard`)
- `delete_records` (variants, codes, prices, relations, rules, documents)

All are batch tools and run in a transaction. Write tools carry MCP annotations
`readOnlyHint=false` and `destructiveHint=true`. They are intended to be
restricted through Agent skills or OpenCode permissions.

## OpenCode connection

Remote Streamable HTTP:

```bash
opencode mcp add pricebook --url http://127.0.0.1:8787/mcp
```

Local stdio (OpenCode starts and stops the process):

```bash
opencode mcp add pricebook -- /absolute/path/to/pricebook-mcp --transport stdio --db /absolute/path/to/data/pricebook.db
```

Or in `opencode.jsonc`:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "pricebook": {
        "type": "remote",
        "url": "http://127.0.0.1:8787/mcp",
        "oauth": false
      }
    }
  }
}
```

Use `"type": "local"` with `command` instead of `type`/`url` for stdio:

```jsonc
{
  "mcp": {
    "servers": {
      "pricebook": {
        "type": "local",
        "command": [
          "/absolute/path/to/pricebook-mcp",
          "--transport", "stdio",
          "--db", "/absolute/path/to/data/pricebook.db"
        ]
      }
    }
  }
}
```

The server binds to loopback and currently has no authentication. Do not expose
it publicly without adding authentication.

## Database

- JSON fields are SQLite JSONB BLOBs.
- Money is stored as exact decimal text in `prices.amount_decimal`.
- Factory and retail prices are separate rows.
- Product codes are aliases, not primary keys.
- Each imported workbook is isolated as a `price_books` row.
- No Excel file, sheet, row or cell references are stored.

## Import tooling

`tools/pricebook_db.py` contains low-level insert/lookup helpers for
human/Agent-reviewed imports. It does not parse spreadsheets automatically.
Use it through `uv`:

```bash
uv run python your_import_script.py
```

## Next steps

1. Add an end-to-end design-drawing quoting test with an Agent.
2. Add an optional deterministic arithmetic helper for line totals.
3. Decide whether to import catalogue images. The current quoting flow does not
   depend on them.
