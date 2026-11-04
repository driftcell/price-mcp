-- PriceBook MCP schema v1.
-- JSON columns are stored as SQLite JSONB BLOBs. Importers must insert them
-- with jsonb(?) (or SQLx bind a JSONB byte sequence), never plain JSON text.

PRAGMA foreign_keys = ON;

CREATE TABLE price_books (
    id              INTEGER PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    series          TEXT NOT NULL,
    product_domain  TEXT NOT NULL CHECK (product_domain IN ('kitchen', 'home')),
    version_label   TEXT NOT NULL,
    currency        TEXT NOT NULL DEFAULT 'CNY',
    status          TEXT NOT NULL DEFAULT 'draft'
                    CHECK (status IN ('draft', 'active', 'retired')),
    effective_from  TEXT,
    effective_to    TEXT,
    notes           TEXT,
    created_at      TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (series, product_domain, version_label)
) STRICT;

CREATE TABLE categories (
    id          INTEGER PRIMARY KEY,
    code        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    parent_id   INTEGER REFERENCES categories(id) ON DELETE RESTRICT,
    sort_order  INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO categories (code, name, sort_order) VALUES
    ('standard_cabinet', '标准单元柜', 10),
    ('cabinet_and_door', '柜体与门板', 20),
    ('door', '门板', 30),
    ('panel', '板件与背板', 40),
    ('countertop', '台面', 50),
    ('decorative', '装饰部件', 60),
    ('hinge', '铰链', 70),
    ('push_opener', '反弹器', 75),
    ('handle', '拉手', 80),
    ('basket', '拉篮', 85),
    ('drawer', '抽屉与导轨', 90),
    ('sink', '水槽', 100),
    ('faucet', '龙头', 110),
    ('toe_kick', '踢脚线', 120),
    ('lighting', '灯光电气', 130),
    ('storage', '收纳五金', 140),
    ('wardrobe', '家居与衣柜五金', 150),
    ('bathroom', '卫浴与阳台', 160),
    ('wine_cabinet', '酒柜', 170),
    ('appliance', '电器', 180),
    ('functional_component', '功能柜体配件', 190),
    ('service', '服务与工艺费', 200),
    ('bundle', '套餐与成品', 210),
    ('other', '其他', 999);

CREATE TABLE products (
    id           INTEGER PRIMARY KEY,
    category_id  INTEGER NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
    product_type TEXT NOT NULL DEFAULT 'item'
                 CHECK (product_type IN ('item', 'system', 'bundle', 'service', 'component', 'master_data')),
    name         TEXT NOT NULL,
    description  TEXT,
    properties   BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(properties, 4)),
    status       TEXT NOT NULL DEFAULT 'unknown'
                 CHECK (status IN ('active', 'planned', 'discontinued', 'unknown')),
    valid_from   TEXT,
    valid_to     TEXT,
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;

CREATE INDEX idx_products_category ON products(category_id);
CREATE INDEX idx_products_name ON products(name);
CREATE INDEX idx_products_status ON products(status);

CREATE TABLE product_scopes (
    product_id      INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    series          TEXT NOT NULL,
    product_domain  TEXT NOT NULL CHECK (product_domain IN ('kitchen', 'home')),
    PRIMARY KEY (product_id, series, product_domain)
) STRICT;

CREATE INDEX idx_product_scopes_lookup ON product_scopes(series, product_domain, product_id);

CREATE TABLE product_variants (
    id          INTEGER PRIMARY KEY,
    product_id  INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    variant_key TEXT NOT NULL,
    label       TEXT,
    properties  BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(properties, 4)),
    constraints BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(constraints, 4)),
    status      TEXT NOT NULL DEFAULT 'unknown'
                CHECK (status IN ('active', 'planned', 'discontinued', 'unknown')),
    valid_from  TEXT,
    valid_to    TEXT,
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (product_id, variant_key)
) STRICT;

CREATE INDEX idx_product_variants_product ON product_variants(product_id);
CREATE INDEX idx_product_variants_status ON product_variants(status);

CREATE TABLE product_codes (
    id          INTEGER PRIMARY KEY,
    product_id  INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    variant_id  INTEGER REFERENCES product_variants(id) ON DELETE CASCADE,
    code        TEXT NOT NULL COLLATE NOCASE,
    code_type   TEXT NOT NULL DEFAULT 'new'
                CHECK (code_type IN ('new', 'old', 'erp', 'model', 'alias', 'legacy')),
    is_primary  INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    conditions  BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(conditions, 4)),
    notes       TEXT
) STRICT;

CREATE UNIQUE INDEX uq_product_codes_exact
    ON product_codes(code, code_type, product_id, ifnull(variant_id, 0));
CREATE INDEX idx_product_codes_code ON product_codes(code);
CREATE INDEX idx_product_codes_product ON product_codes(product_id, variant_id);

CREATE TABLE colors (
    id          INTEGER PRIMARY KEY,
    code        TEXT NOT NULL COLLATE NOCASE UNIQUE,
    name        TEXT NOT NULL,
    color_class TEXT,
    material    TEXT,
    properties  BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(properties, 4)),
    status      TEXT NOT NULL DEFAULT 'unknown'
                CHECK (status IN ('active', 'planned', 'discontinued', 'unknown')),
    valid_from  TEXT,
    valid_to    TEXT,
    notes       TEXT,
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;

CREATE INDEX idx_colors_name ON colors(name);
CREATE INDEX idx_colors_class ON colors(color_class);

CREATE TABLE color_aliases (
    id        INTEGER PRIMARY KEY,
    color_id  INTEGER NOT NULL REFERENCES colors(id) ON DELETE CASCADE,
    alias     TEXT NOT NULL COLLATE NOCASE,
    alias_type TEXT NOT NULL DEFAULT 'alias'
               CHECK (alias_type IN ('alias', 'old_name', 'spelling')),
    UNIQUE (color_id, alias, alias_type)
) STRICT;

CREATE INDEX idx_color_aliases_alias ON color_aliases(alias);

CREATE TABLE pricing_rules (
    id             INTEGER PRIMARY KEY,
    price_book_id  INTEGER NOT NULL REFERENCES price_books(id) ON DELETE CASCADE,
    product_id     INTEGER REFERENCES products(id) ON DELETE SET NULL,
    variant_id     INTEGER REFERENCES product_variants(id) ON DELETE SET NULL,
    category_id    INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    code           TEXT,
    name           TEXT NOT NULL,
    rule_type      TEXT NOT NULL
                   CHECK (rule_type IN (
                       'pricing', 'coefficient', 'tier', 'minimum', 'quantity',
                       'allowance', 'selection', 'inclusion', 'compatibility',
                       'service', 'note'
                   )),
    pricing_mode   TEXT,
    scope          BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(scope, 4)),
    conditions     BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(conditions, 4)),
    data           BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(data, 4)),
    description    TEXT,
    priority       INTEGER NOT NULL DEFAULT 100,
    valid_from     TEXT,
    valid_to       TEXT,
    created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;

CREATE INDEX idx_pricing_rules_book ON pricing_rules(price_book_id, category_id, rule_type);
CREATE INDEX idx_pricing_rules_product ON pricing_rules(product_id, variant_id);

CREATE TABLE prices (
    id                  INTEGER PRIMARY KEY,
    price_book_id       INTEGER NOT NULL REFERENCES price_books(id) ON DELETE CASCADE,
    variant_id          INTEGER NOT NULL REFERENCES product_variants(id) ON DELETE CASCADE,
    price_kind          TEXT NOT NULL
                        CHECK (price_kind IN ('factory', 'retail', 'service_fee', 'reference', 'other')),
    amount_decimal      TEXT,
    currency            TEXT NOT NULL DEFAULT 'CNY',
    unit                TEXT NOT NULL
                        CHECK (unit IN ('piece', 'set', 'pair', 'meter', 'square_meter', 'project')),
    unit_label          TEXT,
    measurement_basis   TEXT NOT NULL DEFAULT 'fixed'
                        CHECK (measurement_basis IN ('fixed', 'length', 'actual_area', 'projected_area', 'expanded_area', 'custom')),
    pricing_mode        TEXT
                        CHECK (pricing_mode IN ('unit_cabinet', 'linear_meter', 'component', 'bundle', 'service', 'other')),
    conditions          BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(conditions, 4)),
    derivation_rule_id  INTEGER REFERENCES pricing_rules(id) ON DELETE SET NULL,
    notes               TEXT,
    valid_from          TEXT,
    valid_to            TEXT,
    created_at          TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (amount_decimal IS NOT NULL OR derivation_rule_id IS NOT NULL)
) STRICT;

CREATE INDEX idx_prices_variant ON prices(variant_id, price_kind);
CREATE INDEX idx_prices_book ON prices(price_book_id, price_kind);
CREATE INDEX idx_prices_mode ON prices(price_book_id, pricing_mode);

CREATE TABLE product_relations (
    id                 INTEGER PRIMARY KEY,
    price_book_id       INTEGER NOT NULL REFERENCES price_books(id) ON DELETE CASCADE,
    parent_product_id   INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    parent_variant_id   INTEGER REFERENCES product_variants(id) ON DELETE CASCADE,
    child_product_id    INTEGER REFERENCES products(id) ON DELETE SET NULL,
    child_variant_id    INTEGER REFERENCES product_variants(id) ON DELETE SET NULL,
    child_label         TEXT,
    relation_type       TEXT NOT NULL
                        CHECK (relation_type IN (
                            'included', 'required', 'optional', 'excluded',
                            'compatible_with', 'incompatible_with', 'upgrade',
                            'replaced_by'
                        )),
    quantity            BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(quantity, 4)),
    conditions          BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(conditions, 4)),
    notes               TEXT,
    CHECK (child_product_id IS NOT NULL OR child_variant_id IS NOT NULL OR child_label IS NOT NULL)
) STRICT;

CREATE INDEX idx_product_relations_parent
    ON product_relations(price_book_id, parent_product_id, parent_variant_id, relation_type);
CREATE INDEX idx_product_relations_child
    ON product_relations(price_book_id, child_product_id, child_variant_id);

CREATE TABLE documents (
    id             INTEGER PRIMARY KEY,
    price_book_id  INTEGER REFERENCES price_books(id) ON DELETE CASCADE,
    category_id    INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    doc_type       TEXT NOT NULL
                   CHECK (doc_type IN (
                       'process', 'policy', 'changelog', 'brand', 'pricing_note',
                       'installation', 'service', 'other'
                   )),
    title          TEXT NOT NULL,
    content        TEXT,
    data           BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(data, 4)),
    sort_order     INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;

CREATE INDEX idx_documents_book ON documents(price_book_id, doc_type, category_id);

CREATE TABLE images (
    id          INTEGER PRIMARY KEY,
    sha256      TEXT NOT NULL UNIQUE,
    mime_type   TEXT NOT NULL,
    width       INTEGER,
    height      INTEGER,
    byte_size   INTEGER NOT NULL,
    data        BLOB NOT NULL,
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
) STRICT;

CREATE TABLE image_links (
    id           INTEGER PRIMARY KEY,
    image_id     INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE,
    owner_type   TEXT NOT NULL
                 CHECK (owner_type IN ('price_book', 'product', 'variant', 'color', 'pricing_rule', 'document')),
    owner_id     INTEGER NOT NULL,
    role         TEXT NOT NULL DEFAULT 'product_photo'
                 CHECK (role IN (
                     'product_photo', 'color_swatch', 'door_style', 'diagram',
                     'installation', 'process_diagram', 'change_evidence', 'cover', 'other'
                 )),
    sort_order   INTEGER NOT NULL DEFAULT 0,
    is_primary   INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    crop         BLOB NOT NULL DEFAULT (jsonb('{}')) CHECK (json_valid(crop, 4)),
    alt_text     TEXT,
    UNIQUE (image_id, owner_type, owner_id, role, sort_order)
) STRICT;

CREATE INDEX idx_image_links_owner ON image_links(owner_type, owner_id, role, sort_order);

CREATE VIEW v_product_catalog AS
SELECT
    p.id                AS product_id,
    p.name              AS product_name,
    c.code              AS category_code,
    c.name              AS category_name,
    p.product_type,
    p.status            AS product_status,
    (
        SELECT group_concat(pc.code, ', ')
        FROM product_codes pc
        WHERE pc.product_id = p.id AND pc.is_primary = 1
    )                   AS primary_codes,
    (
        SELECT group_concat(ps.series, '/')
        FROM product_scopes ps
        WHERE ps.product_id = p.id
    )                   AS series,
    json(p.properties)  AS properties
FROM products p
JOIN categories c ON c.id = p.category_id;
