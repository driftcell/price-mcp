# PriceBook import guide

This database is the authoritative product and price source. The original Excel
files are not stored in or referenced from the database.

## Fixed scope

- Pricebook series: `J`, `P`, `S`.
- Product domains: `kitchen`, `home`.
- Workbook-to-price-book mapping:

| Workbook | slug | series | domain |
|---|---|---|---|
| `J系列橱柜价格10.15.xlsx` | `j-kitchen-10.15` | `J` | `kitchen` |
| `P系列家居柜价格10.15.xlsx` | `p-home-10.15` | `P` | `home` |
| `P系列橱柜价格10.15.xlsx` | `p-kitchen-10.15` | `P` | `kitchen` |
| `S系列家居柜价格10.15.xlsx` | `s-home-10.15` | `S` | `home` |
| `S系列橱柜价格10.15.xlsx` | `s-kitchen-10.15` | `S` | `kitchen` |

Create each price book with status `draft`. Mark it `active` only after the
import checks below pass. Never mix prices from one workbook into another price
book.

## Database access

Database file:

```text
/Users/driftcell/Desktop/PriceBookMCP/data/pricebook.db
```

Use Python through `uv run` so JSONB is supported:

```bash
uv run python your_import_script.py
```

Use the provided low-level helpers:

```python
from tools.pricebook_db import connect, create_price_book
```

Always enable foreign keys in a raw connection:

```python
con = connect()
con.execute("PRAGMA foreign_keys = ON")
```

JSON columns are SQLite JSONB BLOBs. Never insert JSON text directly into them.
The helper does this correctly:

```python
from tools.pricebook_db import jsonb
properties = jsonb(con, {"width_mm": 600, "height_mm": 720})
```

Commit each completed price book in one final transaction if practical. If the
import is long, commit in coherent batches and leave the price book in `draft`
until complete.

## Do not store

Never store or add columns for:

- Excel file names or paths
- Sheet names, row numbers or cell addresses
- File hashes or import source pointers
- `DISPIMG(...)` formulas
- Raw whole-row JSON copies

Cell addresses may be used in temporary working notes for verification, but
must not enter the database.

## Product identity

A product code is not a unique key.

- `products` is the model/product family.
- `product_variants` is a physical configuration that a customer can select.
- `product_codes` stores new, old, ERP, aliases and conditional mappings.
- Prices always attach to a variant.

Rules:

1. Search existing products by code before creating one:
   ```python
   from tools.pricebook_db import find_products_by_code
   find_products_by_code(con, "PGF-A1001")
   ```
2. Reuse an existing product only when it is clearly the same physical model.
   If the same code represents different products, keep separate product rows;
   the code table intentionally allows that.
3. Reuse an existing variant when its physical configuration is identical.
   Add this price book's prices to that variant.
4. Create separate variants for distinct dimensions, opening direction, capacity,
   NL, thickness/configuration, or other properties that change what the item is.
5. Do not create separate variants merely because there is another price tier.
   Price-only dimensions belong in `prices.conditions`. Examples: single/double
   sided, color class, front width tier, hand orientation used only for pricing.
6. Shared products used by both P and S series get both scopes:
   `S` and `P`. Do not store `"S/P"` as one series value.
7. A price book may reuse a product created by an earlier import. Do not alter
   earlier price rows.

Suggested `variant_key`: a stable, lowercase, ASCII signature such as
`w600-h700-d430-left`. Use `default` if there is genuinely one configuration.
The key only needs to be unique within its product.

## Categories

Use one of these category codes:

```text
standard_cabinet       cabinet_and_door       door
panel                  countertop             decorative
hinge                  push_opener            handle
basket                 drawer                 sink
faucet                 toe_kick               lighting
storage                wardrobe               bathroom
wine_cabinet           appliance              functional_component
service                bundle                 other
```

Set `product_type` to one of:
`item`, `system`, `bundle`, `service`, `component`, `master_data`.

Use `service` for process charges and standalone services. Use `bundle` for
genuine package products. Use `component` for parts that primarily participate
in another product's BOM.

## Prices

Every price row has:

- `price_kind`: `factory`, `retail`, `service_fee`, `reference`, `other`
- canonical `unit`: `piece`, `set`, `pair`, `meter`, `square_meter`, `project`
- `unit_label`: the human-readable original unit, e.g. `元/㎡`, `元/延米`
- `measurement_basis`: `fixed`, `length`, `actual_area`, `projected_area`,
  `expanded_area`, `custom`
- optional `pricing_mode`: `unit_cabinet`, `linear_meter`, `component`,
  `bundle`, `service`, `other`
- `amount_decimal`: exact decimal string, no currency symbol or commas
- `conditions`: the conditions under which this price applies

Always store factory and retail as separate rows. Never derive one from the
other with a global multiplier.

`0` means genuinely free. If a price is included in another product, do not add
a zero-price row: record an `included` relation. If a product/option cannot be
offered, do not add a row; record an exclusion relation or explanatory pricing
rule.

If a price is defined as another price times a factor, create a pricing rule
with `rule_type='pricing'` and pass its ID as `derivation_rule_id` to
`add_price`. Do not put a formula in `amount_decimal`.

## JSON conventions

Use normalized units and snake_case keys. The following keys should be
consistent across all imports.

### Common `properties` keys

| Key | Meaning |
|---|---|
| `spec_text` | human-readable normalized specification |
| `width_mm`, `height_mm`, `depth_mm`, `length_mm` | dimensions |
| `thickness_mm` | material thickness |
| `inner_width_mm`, `inner_depth_mm`, `inner_height_mm` | internal dimensions |
| `cabinet_width_mm` | compatible cabinet width |
| `front_width_mm` | drawer/door front width |
| `nl_mm` | drawer nominal length |
| `color_class` | color/pricing class such as `纯色A类` |
| `material` | material |
| `finish` | surface treatment |
| `application` | usage such as `平开门`, `抽面`, `见光板` |
| `opening` | opening configuration |
| `direction` | `left`, `right`, `double`, `top`, `bottom` |
| `side` | `single` or `double` for sidedness |
| `angle_deg` | hinge/opening angle |
| `power_w` | electrical power |
| `capacity_kg` | load capacity |
| `lifecycle_note` | status/effective-date wording normalized into notes |

Use numbers for numeric measurements and arrays for multiple values. Do not use
Chinese unit suffixes in property values.

### `conditions` matcher shape

`conditions` is a JSON object. All keys are AND conditions.

- Exact match:
  ```json
  {"application": "平开门", "color_class": "纯色A类"}
  ```
- Enumeration:
  ```json
  {"color_class": {"in": ["纯色B类", "纯色C类"]}}
  ```
- Numeric range:
  ```json
  {
    "front_width_mm": {
      "min": 600,
      "max": 1000,
      "include_min": false,
      "include_max": true,
      "exclude": [605]
    }
  }
  ```
- Empty object means no extra condition.

Write one row per OR branch. Never duplicate the same condition in one price
row. Record boundary interpretation explicitly when the source is ambiguous.

### `constraints` shape

Use a nested object without executing it. Example:

```json
{
  "applications": {
    "平开门": {
      "width_mm": {"min": 200, "max": 600},
      "height_mm": {"min": 160, "max": 2800}
    }
  },
  "allowed_color_classes": ["纯色A类", "纯色A1类"],
  "hard": true
}
```

Use `"hard": false` for recommendations. Do not turn installation advice into a
hard constraint unless the source says so.

### `pricing_rules.data`

No executable rule engine is required. Keep data typed enough for an Agent to
read and apply consistently.

Coefficient example:

```json
{
  "dimension": "height_mm",
  "brackets": [
    {
      "min": 701,
      "max": 800,
      "include_min": true,
      "include_max": true,
      "exclude": [720, 780],
      "coefficient": "1.2"
    }
  ]
}
```

Minimum/step example:

```json
{
  "minimum_quantity": "0.4",
  "quantity_step": null,
  "unit": "square_meter",
  "rounding": "up"
}
```

Tier example:

```json
{
  "dimension": "front_width_mm",
  "brackets": [
    {"max": 600, "include_max": true, "label": "≤600"},
    {"min": 600, "max": 1000, "include_min": false, "include_max": true, "label": "600–1000"}
  ]
}
```

## Relations

Use `product_relations` for cross-product semantics. Do not bury them only in
free text.

- `included`: already included in the parent price
- `required`: required but separately priced or separately selected
- `optional`: optional accessory
- `excluded`: explicitly not included
- `compatible_with` / `incompatible_with`: fit constraints
- `upgrade`: an upgrade/replacement with a difference
- `replaced_by`: lifecycle replacement

Use `child_label` only when no child product/variant exists. A quote must not
invent a price for a label-only relation.

## Documents

Use `documents` for non-product text and structured reference data:

- basic process/default configuration
- non-priced service policy
- changelog/version notes
- brand introductions
- installation notes
- pricing notes that are better shown as a document than as a rule

Do not duplicate a real price only inside a document.

## Images

Image storage is available but not a blocker for the first data import. The
Agent quoting flow sees customer design drawings, not the catalogue images.
If images are imported later:

- Deduplicate by SHA-256.
- Store original bytes in `images.data`.
- Link via `image_links`.
- Use correct roles; never attach changelog screenshots as product photos.

## Import checks

Before marking the price book `active`, run and report:

1. Row counts by table for this price book.
2. Every price's variant belongs to the intended product.
3. Every `product_codes.variant_id`, when present, belongs to its product.
4. `typeof(properties/constraints/conditions/data/crop) = 'blob'` for all JSONB.
5. `json_valid(..., 4) = 1` for every JSONB value.
6. No price with a currency symbol, comma or non-numeric `amount_decimal`.
7. All selected products have at least one scope matching the price book series
   and domain.
8. Spot-check at least five products/prices against the workbook, including:
   - one multiple-price product
   - one tiered/range product
   - one service or minimum-charge case
   - one included/optional relation
   - one lifecycle/status case
9. Foreign key check:
   ```sql
   PRAGMA foreign_key_check;
   ```
10. No source file/sheet/row/cell data was written.

Report in the final response:

- price book ID and status
- product, variant, code, price, rule, relation, document counts
- notable identity merges with earlier price books
- unresolved ambiguities or intentional omissions
- five spot-checked examples
- any checks not passed
