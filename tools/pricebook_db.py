"""Low-level helpers for one-off, human/agent-reviewed PriceBook imports.

This module deliberately does not parse spreadsheets. Each importer decides
which rows mean what, then calls these helpers with already normalized values.
"""

from __future__ import annotations

import json
import os
import sqlite3
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DB = REPO_ROOT / "data" / "pricebook.db"


def db_path() -> Path:
    return Path(os.environ.get("PRICEBOOK_DB", DEFAULT_DB)).resolve()


def connect(path: str | Path | None = None) -> sqlite3.Connection:
    connection = sqlite3.connect(path or db_path(), timeout=30)
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA foreign_keys = ON")
    connection.execute("PRAGMA busy_timeout = 30000")
    return connection


def jsonb(connection: sqlite3.Connection, value: Any) -> bytes:
    """Return the canonical SQLite JSONB representation of a Python value."""
    encoded = json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    )
    return connection.execute("SELECT jsonb(?)", (encoded,)).fetchone()[0]


def create_price_book(
    connection: sqlite3.Connection,
    *,
    slug: str,
    name: str,
    series: str,
    product_domain: str,
    version_label: str,
    currency: str = "CNY",
    effective_from: str | None = None,
    effective_to: str | None = None,
    notes: str | None = None,
) -> int:
    cursor = connection.execute(
        """
        INSERT INTO price_books
            (slug, name, series, product_domain, version_label, currency,
             status, effective_from, effective_to, notes)
        VALUES (?, ?, ?, ?, ?, ?, 'draft', ?, ?, ?)
        """,
        (
            slug,
            name,
            series,
            product_domain,
            version_label,
            currency,
            effective_from,
            effective_to,
            notes,
        ),
    )
    return int(cursor.lastrowid)


def set_price_book_status(
    connection: sqlite3.Connection, price_book_id: int, status: str
) -> None:
    if status not in {"draft", "active", "retired"}:
        raise ValueError(f"invalid price book status: {status}")
    connection.execute(
        "UPDATE price_books SET status = ? WHERE id = ?", (status, price_book_id)
    )


def category_id(connection: sqlite3.Connection, code: str) -> int:
    row = connection.execute(
        "SELECT id FROM categories WHERE code = ?", (code,)
    ).fetchone()
    if row is None:
        raise KeyError(f"unknown category code: {code}")
    return int(row["id"])


def create_product(
    connection: sqlite3.Connection,
    *,
    category_code: str,
    name: str,
    product_type: str = "item",
    description: str | None = None,
    properties: Any | None = None,
    status: str = "unknown",
    valid_from: str | None = None,
    valid_to: str | None = None,
) -> int:
    cursor = connection.execute(
        """
        INSERT INTO products
            (category_id, product_type, name, description, properties,
             status, valid_from, valid_to)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            category_id(connection, category_code),
            product_type,
            name,
            description,
            jsonb(connection, properties or {}),
            status,
            valid_from,
            valid_to,
        ),
    )
    return int(cursor.lastrowid)


def add_product_scope(
    connection: sqlite3.Connection,
    product_id: int,
    *,
    series: str,
    product_domain: str,
) -> None:
    connection.execute(
        """
        INSERT OR IGNORE INTO product_scopes (product_id, series, product_domain)
        VALUES (?, ?, ?)
        """,
        (product_id, series, product_domain),
    )


def create_variant(
    connection: sqlite3.Connection,
    *,
    product_id: int,
    variant_key: str,
    label: str | None = None,
    properties: Any | None = None,
    constraints: Any | None = None,
    status: str = "unknown",
    valid_from: str | None = None,
    valid_to: str | None = None,
) -> int:
    cursor = connection.execute(
        """
        INSERT INTO product_variants
            (product_id, variant_key, label, properties, constraints,
             status, valid_from, valid_to)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            product_id,
            variant_key,
            label,
            jsonb(connection, properties or {}),
            jsonb(connection, constraints or {}),
            status,
            valid_from,
            valid_to,
        ),
    )
    return int(cursor.lastrowid)


def add_product_code(
    connection: sqlite3.Connection,
    *,
    product_id: int,
    code: str,
    variant_id: int | None = None,
    code_type: str = "new",
    is_primary: bool = False,
    conditions: Any | None = None,
    notes: str | None = None,
) -> int:
    cursor = connection.execute(
        """
        INSERT INTO product_codes
            (product_id, variant_id, code, code_type, is_primary, conditions, notes)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        """,
        (
            product_id,
            variant_id,
            code,
            code_type,
            int(is_primary),
            jsonb(connection, conditions or {}),
            notes,
        ),
    )
    return int(cursor.lastrowid)


def find_products_by_code(connection: sqlite3.Connection, code: str) -> list[dict]:
    rows = connection.execute(
        """
        SELECT
            p.id AS product_id,
            p.name,
            c.code AS category_code,
            p.product_type,
            p.status,
            pc.code,
            pc.code_type,
            pc.variant_id,
            json(p.properties) AS properties
        FROM product_codes pc
        JOIN products p ON p.id = pc.product_id
        JOIN categories c ON c.id = p.category_id
        WHERE pc.code = ? COLLATE NOCASE
        ORDER BY p.id
        """,
        (code,),
    ).fetchall()
    return [dict(row) for row in rows]


def create_color(
    connection: sqlite3.Connection,
    *,
    code: str,
    name: str,
    color_class: str | None = None,
    material: str | None = None,
    properties: Any | None = None,
    status: str = "unknown",
    valid_from: str | None = None,
    valid_to: str | None = None,
    notes: str | None = None,
    aliases: list[str] | None = None,
) -> int:
    cursor = connection.execute(
        """
        INSERT INTO colors
            (code, name, color_class, material, properties, status,
             valid_from, valid_to, notes)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            code,
            name,
            color_class,
            material,
            jsonb(connection, properties or {}),
            status,
            valid_from,
            valid_to,
            notes,
        ),
    )
    color_id = int(cursor.lastrowid)
    for alias in aliases or []:
        connection.execute(
            "INSERT INTO color_aliases (color_id, alias, alias_type) VALUES (?, ?, 'alias')",
            (color_id, alias),
        )
    return color_id


def add_pricing_rule(
    connection: sqlite3.Connection,
    *,
    price_book_id: int,
    name: str,
    rule_type: str,
    product_id: int | None = None,
    variant_id: int | None = None,
    category_code: str | None = None,
    code: str | None = None,
    pricing_mode: str | None = None,
    scope: Any | None = None,
    conditions: Any | None = None,
    data: Any | None = None,
    description: str | None = None,
    priority: int = 100,
    valid_from: str | None = None,
    valid_to: str | None = None,
) -> int:
    cursor = connection.execute(
        """
        INSERT INTO pricing_rules
            (price_book_id, product_id, variant_id, category_id, code, name,
             rule_type, pricing_mode, scope, conditions, data, description,
             priority, valid_from, valid_to)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            price_book_id,
            product_id,
            variant_id,
            category_id(connection, category_code) if category_code else None,
            code,
            name,
            rule_type,
            pricing_mode,
            jsonb(connection, scope or {}),
            jsonb(connection, conditions or {}),
            jsonb(connection, data or {}),
            description,
            priority,
            valid_from,
            valid_to,
        ),
    )
    return int(cursor.lastrowid)


def add_price(
    connection: sqlite3.Connection,
    *,
    price_book_id: int,
    variant_id: int,
    price_kind: str,
    unit: str,
    measurement_basis: str = "fixed",
    pricing_mode: str | None = None,
    amount_decimal: str | int | float | None = None,
    unit_label: str | None = None,
    currency: str = "CNY",
    conditions: Any | None = None,
    derivation_rule_id: int | None = None,
    notes: str | None = None,
    valid_from: str | None = None,
    valid_to: str | None = None,
) -> int:
    if amount_decimal is not None and not isinstance(amount_decimal, str):
        amount_decimal = str(amount_decimal)
    if amount_decimal is None and derivation_rule_id is None:
        raise ValueError("price requires amount_decimal or derivation_rule_id")
    cursor = connection.execute(
        """
        INSERT INTO prices
            (price_book_id, variant_id, price_kind, amount_decimal, currency,
             unit, unit_label, measurement_basis, pricing_mode, conditions,
             derivation_rule_id, notes, valid_from, valid_to)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            price_book_id,
            variant_id,
            price_kind,
            amount_decimal,
            currency,
            unit,
            unit_label,
            measurement_basis,
            pricing_mode,
            jsonb(connection, conditions or {}),
            derivation_rule_id,
            notes,
            valid_from,
            valid_to,
        ),
    )
    return int(cursor.lastrowid)


def add_relation(
    connection: sqlite3.Connection,
    *,
    price_book_id: int,
    parent_product_id: int,
    relation_type: str,
    parent_variant_id: int | None = None,
    child_product_id: int | None = None,
    child_variant_id: int | None = None,
    child_label: str | None = None,
    quantity: Any | None = None,
    conditions: Any | None = None,
    notes: str | None = None,
) -> int:
    cursor = connection.execute(
        """
        INSERT INTO product_relations
            (price_book_id, parent_product_id, parent_variant_id,
             child_product_id, child_variant_id, child_label, relation_type,
             quantity, conditions, notes)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            price_book_id,
            parent_product_id,
            parent_variant_id,
            child_product_id,
            child_variant_id,
            child_label,
            relation_type,
            jsonb(connection, quantity or {}),
            jsonb(connection, conditions or {}),
            notes,
        ),
    )
    return int(cursor.lastrowid)


def add_document(
    connection: sqlite3.Connection,
    *,
    doc_type: str,
    title: str,
    content: str | None = None,
    price_book_id: int | None = None,
    category_code: str | None = None,
    data: Any | None = None,
    sort_order: int = 0,
) -> int:
    cursor = connection.execute(
        """
        INSERT INTO documents
            (price_book_id, category_id, doc_type, title, content, data, sort_order)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        """,
        (
            price_book_id,
            category_id(connection, category_code) if category_code else None,
            doc_type,
            title,
            content,
            jsonb(connection, data or {}),
            sort_order,
        ),
    )
    return int(cursor.lastrowid)


def add_image(
    connection: sqlite3.Connection,
    *,
    sha256: str,
    mime_type: str,
    data: bytes,
    owner_type: str,
    owner_id: int,
    role: str = "product_photo",
    width: int | None = None,
    height: int | None = None,
    sort_order: int = 0,
    is_primary: bool = False,
    crop: Any | None = None,
    alt_text: str | None = None,
) -> int:
    connection.execute(
        """
        INSERT INTO images (sha256, mime_type, width, height, byte_size, data)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(sha256) DO NOTHING
        """,
        (sha256, mime_type, width, height, len(data), data),
    )
    image_id = int(
        connection.execute(
            "SELECT id FROM images WHERE sha256 = ?", (sha256,)
        ).fetchone()["id"]
    )
    connection.execute(
        """
        INSERT INTO image_links
            (image_id, owner_type, owner_id, role, sort_order, is_primary,
             crop, alt_text)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(image_id, owner_type, owner_id, role, sort_order) DO NOTHING
        """,
        (
            image_id,
            owner_type,
            owner_id,
            role,
            sort_order,
            int(is_primary),
            jsonb(connection, crop or {}),
            alt_text,
        ),
    )
    return image_id
