use std::collections::HashSet;

use crate::db::{self, Row, WriteBatch, resolve_ids};
use crate::store::PriceBookStore;
use crate::write_models::{
    DeleteProductsRequest, DeleteRecordsRequest, DeleteTable, SaveCodesRequest,
    SaveDocumentsRequest, SavePricesRequest, SavePricingRulesRequest, SaveProductsRequest,
    SaveRelationsRequest, SaveVariantsRequest, WriteResult,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub async fn save_products(
    store: &PriceBookStore,
    request: SaveProductsRequest,
) -> Result<WriteResult> {
    ensure_batch(&request.items)?;
    ensure_unique_ids(request.items.iter().map(|item| item.product_id))?;
    let mut tx = store.write_batch();
    let mut created = Vec::new();
    let mut updated = Vec::new();
    let mut next_product_id = None;

    for item in request.items {
        if let Some(product_id) = item.product_id {
            update_product(&mut tx, product_id, item).await?;
            updated.push(product_id);
        } else {
            let category_code = required_string(item.category_code, "category_code")?;
            let name = required_string(item.name, "name")?;
            let scopes = item
                .scopes
                .context("scopes are required when creating a product")?;
            if scopes.is_empty() {
                bail!("at least one scope is required when creating a product");
            }
            let category_id = category_id(&mut tx, &category_code).await?;
            let properties = json_text(item.properties.unwrap_or_else(|| json!({})))?;
            let product_id: i64 = match next_product_id {
                Some(id) => id,
                None => db::query("SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM products")
                    .fetch_one(&tx)
                    .await?
                    .get("next_id"),
            };
            next_product_id = Some(
                product_id
                    .checked_add(1)
                    .context("product ID space is exhausted")?,
            );
            db::query(
                "INSERT INTO products \
                    (id, category_id, product_type, name, description, properties, status, valid_from, valid_to) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(product_id)
            .bind(category_id)
            .bind(item.product_type.unwrap_or_else(|| "item".to_owned()))
            .bind(name)
            .bind(item.description)
            .bind(properties)
            .bind(item.status.unwrap_or_else(|| "unknown".to_owned()))
            .bind(item.valid_from)
            .bind(item.valid_to)
            .execute(&mut tx)
            .await?;
            insert_new_product_scopes(&mut tx, product_id, scopes).await?;
            created.push(product_id);
        }
    }

    tx.commit().await?;
    Ok(WriteResult {
        created,
        updated,
        deleted: Vec::new(),
    })
}

pub async fn save_variants(
    store: &PriceBookStore,
    request: SaveVariantsRequest,
) -> Result<WriteResult> {
    ensure_batch(&request.items)?;
    ensure_unique_ids(request.items.iter().map(|item| item.variant_id))?;
    let mut tx = store.write_batch();
    let mut created = Vec::new();
    let mut updated = Vec::new();

    for item in request.items {
        if let Some(variant_id) = item.variant_id {
            let current = db::query(
                "SELECT product_id, variant_key, label, json(properties) AS properties_json, \
                        json(constraints) AS constraints_json, status, valid_from, valid_to \
                 FROM product_variants WHERE id = ?",
            )
            .bind(variant_id)
            .fetch_optional(&tx)
            .await?
            .with_context(|| format!("variant {variant_id} was not found"))?;
            if let Some(product_id) = item.product_id {
                let current_product: i64 = current.get("product_id");
                if product_id != current_product {
                    bail!("variant product_id cannot be changed");
                }
            }
            let variant_key = match item.variant_key {
                Some(value) => required_nonempty(value, "variant_key")?,
                None => current.get("variant_key"),
            };
            let properties = match item.properties {
                Some(value) => json_text(value)?,
                None => current.get("properties_json"),
            };
            let constraints = match item.constraints {
                Some(value) => json_text(value)?,
                None => current.get("constraints_json"),
            };
            db::query(
                "UPDATE product_variants SET variant_key = ?, label = ?, properties = ?, \
                    constraints = ?, status = ?, valid_from = ?, valid_to = ? WHERE id = ?",
            )
            .bind(variant_key)
            .bind(item.label.or_else(|| current.get("label")))
            .bind(properties)
            .bind(constraints)
            .bind(item.status.unwrap_or_else(|| current.get("status")))
            .bind(item.valid_from.or_else(|| current.get("valid_from")))
            .bind(item.valid_to.or_else(|| current.get("valid_to")))
            .bind(variant_id)
            .execute(&mut tx)
            .await?;
            updated.push(variant_id);
        } else {
            let product_id = item.product_id.context("product_id is required")?;
            let variant_key = required_string(item.variant_key, "variant_key")?;
            let properties = json_text(item.properties.unwrap_or_else(|| json!({})))?;
            let constraints = json_text(item.constraints.unwrap_or_else(|| json!({})))?;
            let variant_id = db::query(
                "INSERT INTO product_variants \
                    (product_id, variant_key, label, properties, constraints, status, valid_from, valid_to) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(product_id)
            .bind(variant_key)
            .bind(item.label)
            .bind(properties)
            .bind(constraints)
            .bind(item.status.unwrap_or_else(|| "unknown".to_owned()))
            .bind(item.valid_from)
            .bind(item.valid_to)
            .execute(&mut tx)
            .await?
            .last_insert_rowid();
            created.push(variant_id);
        }
    }

    let results = tx.commit().await?;
    let created = resolve_ids(created, &results)?;
    Ok(WriteResult {
        created,
        updated,
        deleted: Vec::new(),
    })
}

pub async fn save_codes(store: &PriceBookStore, request: SaveCodesRequest) -> Result<WriteResult> {
    ensure_batch(&request.items)?;
    ensure_unique_ids(request.items.iter().map(|item| item.code_id))?;
    let mut tx = store.write_batch();
    let mut created = Vec::new();
    let mut updated = Vec::new();

    for item in request.items {
        if let Some(code_id) = item.code_id {
            let current = db::query(
                "SELECT product_id, variant_id, code, code_type, is_primary, \
                        json(conditions) AS conditions_json, notes \
                 FROM product_codes WHERE id = ?",
            )
            .bind(code_id)
            .fetch_optional(&tx)
            .await?
            .with_context(|| format!("code row {code_id} was not found"))?;
            if let Some(product_id) = item.product_id {
                let current_product: i64 = current.get("product_id");
                if product_id != current_product {
                    bail!("code product_id cannot be changed");
                }
            }
            let code = match item.code {
                Some(value) => required_nonempty(value, "code")?,
                None => current.get("code"),
            };
            let conditions = match item.conditions {
                Some(value) => json_text(value)?,
                None => current.get("conditions_json"),
            };
            let is_primary = item
                .is_primary
                .map(i64::from)
                .unwrap_or_else(|| current.get("is_primary"));
            let variant_id: Option<i64> = item.variant_id.or_else(|| current.get("variant_id"));
            let product_id: i64 = current.get("product_id");
            validate_code_variant(&mut tx, product_id, variant_id).await?;
            db::query(
                "UPDATE product_codes SET variant_id = ?, code = ?, code_type = ?, \
                    is_primary = ?, conditions = ?, notes = ? WHERE id = ?",
            )
            .bind(variant_id)
            .bind(code)
            .bind(item.code_type.unwrap_or_else(|| current.get("code_type")))
            .bind(is_primary)
            .bind(conditions)
            .bind(item.notes.or_else(|| current.get("notes")))
            .bind(code_id)
            .execute(&mut tx)
            .await?;
            updated.push(code_id);
        } else {
            let product_id = item.product_id.context("product_id is required")?;
            let code = required_string(item.code, "code")?;
            validate_code_variant(&mut tx, product_id, item.variant_id).await?;
            let conditions = json_text(item.conditions.unwrap_or_else(|| json!({})))?;
            let code_id = db::query(
                "INSERT INTO product_codes \
                    (product_id, variant_id, code, code_type, is_primary, conditions, notes) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(product_id)
            .bind(item.variant_id)
            .bind(code)
            .bind(item.code_type.unwrap_or_else(|| "new".to_owned()))
            .bind(item.is_primary.unwrap_or(false) as i64)
            .bind(conditions)
            .bind(item.notes)
            .execute(&mut tx)
            .await?
            .last_insert_rowid();
            created.push(code_id);
        }
    }

    let results = tx.commit().await?;
    let created = resolve_ids(created, &results)?;
    Ok(WriteResult {
        created,
        updated,
        deleted: Vec::new(),
    })
}

pub async fn save_prices(
    store: &PriceBookStore,
    request: SavePricesRequest,
) -> Result<WriteResult> {
    ensure_batch(&request.items)?;
    ensure_unique_ids(request.items.iter().map(|item| item.price_id))?;
    let mut tx = store.write_batch();
    let mut created = Vec::new();
    let mut updated = Vec::new();

    for item in request.items {
        if let Some(price_id) = item.price_id {
            let current = db::query(
                "SELECT price_book_id, variant_id, price_kind, amount_decimal, currency, unit, \
                        unit_label, measurement_basis, pricing_mode, \
                        json(conditions) AS conditions_json, derivation_rule_id, notes, \
                        valid_from, valid_to \
                 FROM prices WHERE id = ?",
            )
            .bind(price_id)
            .fetch_optional(&tx)
            .await?
            .with_context(|| format!("price {price_id} was not found"))?;
            let price_book_id: i64 = item
                .price_book_id
                .unwrap_or_else(|| current.get("price_book_id"));
            let derivation_rule_id: Option<i64> = item
                .derivation_rule_id
                .or_else(|| current.get("derivation_rule_id"));
            let amount_decimal: Option<String> = item
                .amount_decimal
                .or_else(|| current.get("amount_decimal"));
            validate_price_amount(amount_decimal.as_deref())?;
            if amount_decimal.is_none() && derivation_rule_id.is_none() {
                bail!("price requires amount_decimal or derivation_rule_id");
            }
            if let Some(rule_id) = derivation_rule_id {
                validate_rule_book(&mut tx, rule_id, price_book_id).await?;
            }
            let conditions = match item.conditions {
                Some(value) => json_text(value)?,
                None => current.get("conditions_json"),
            };
            db::query(
                "UPDATE prices SET price_book_id = ?, variant_id = ?, price_kind = ?, \
                    amount_decimal = ?, currency = ?, unit = ?, unit_label = ?, \
                    measurement_basis = ?, pricing_mode = ?, conditions = ?, \
                    derivation_rule_id = ?, notes = ?, valid_from = ?, valid_to = ? \
                 WHERE id = ?",
            )
            .bind(price_book_id)
            .bind(item.variant_id.unwrap_or_else(|| current.get("variant_id")))
            .bind(item.price_kind.unwrap_or_else(|| current.get("price_kind")))
            .bind(amount_decimal)
            .bind(item.currency.unwrap_or_else(|| current.get("currency")))
            .bind(item.unit.unwrap_or_else(|| current.get("unit")))
            .bind(item.unit_label.or_else(|| current.get("unit_label")))
            .bind(
                item.measurement_basis
                    .unwrap_or_else(|| current.get("measurement_basis")),
            )
            .bind(item.pricing_mode.or_else(|| current.get("pricing_mode")))
            .bind(conditions)
            .bind(derivation_rule_id)
            .bind(item.notes.or_else(|| current.get("notes")))
            .bind(item.valid_from.or_else(|| current.get("valid_from")))
            .bind(item.valid_to.or_else(|| current.get("valid_to")))
            .bind(price_id)
            .execute(&mut tx)
            .await?;
            updated.push(price_id);
        } else {
            let price_book_id = item.price_book_id.context("price_book_id is required")?;
            let variant_id = item.variant_id.context("variant_id is required")?;
            let price_kind = required_string(item.price_kind, "price_kind")?;
            let unit = required_string(item.unit, "unit")?;
            validate_price_amount(item.amount_decimal.as_deref())?;
            if item.amount_decimal.is_none() && item.derivation_rule_id.is_none() {
                bail!("price requires amount_decimal or derivation_rule_id");
            }
            if let Some(rule_id) = item.derivation_rule_id {
                validate_rule_book(&mut tx, rule_id, price_book_id).await?;
            }
            let conditions = json_text(item.conditions.unwrap_or_else(|| json!({})))?;
            let price_id = db::query(
                "INSERT INTO prices \
                    (price_book_id, variant_id, price_kind, amount_decimal, currency, unit, \
                     unit_label, measurement_basis, pricing_mode, conditions, derivation_rule_id, \
                     notes, valid_from, valid_to) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(price_book_id)
            .bind(variant_id)
            .bind(price_kind)
            .bind(item.amount_decimal)
            .bind(item.currency.unwrap_or_else(|| "CNY".to_owned()))
            .bind(unit)
            .bind(item.unit_label)
            .bind(item.measurement_basis.unwrap_or_else(|| "fixed".to_owned()))
            .bind(item.pricing_mode)
            .bind(conditions)
            .bind(item.derivation_rule_id)
            .bind(item.notes)
            .bind(item.valid_from)
            .bind(item.valid_to)
            .execute(&mut tx)
            .await?
            .last_insert_rowid();
            created.push(price_id);
        }
    }

    let results = tx.commit().await?;
    let created = resolve_ids(created, &results)?;
    Ok(WriteResult {
        created,
        updated,
        deleted: Vec::new(),
    })
}

pub async fn save_relations(
    store: &PriceBookStore,
    request: SaveRelationsRequest,
) -> Result<WriteResult> {
    ensure_batch(&request.items)?;
    ensure_unique_ids(request.items.iter().map(|item| item.relation_id))?;
    let mut tx = store.write_batch();
    let mut created = Vec::new();
    let mut updated = Vec::new();

    for item in request.items {
        if let Some(relation_id) = item.relation_id {
            let current = db::query(
                "SELECT price_book_id, parent_product_id, parent_variant_id, child_product_id, \
                        child_variant_id, child_label, relation_type, json(quantity) AS quantity_json, \
                        json(conditions) AS conditions_json, notes \
                 FROM product_relations WHERE id = ?",
            )
            .bind(relation_id)
            .fetch_optional(&tx)
            .await?
            .with_context(|| format!("relation {relation_id} was not found"))?;
            let quantity = match item.quantity {
                Some(value) => json_text(value)?,
                None => current.get("quantity_json"),
            };
            let conditions = match item.conditions {
                Some(value) => json_text(value)?,
                None => current.get("conditions_json"),
            };
            db::query(
                "UPDATE product_relations SET price_book_id = ?, parent_product_id = ?, \
                    parent_variant_id = ?, child_product_id = ?, child_variant_id = ?, \
                    child_label = ?, relation_type = ?, quantity = ?, \
                    conditions = ?, notes = ? WHERE id = ?",
            )
            .bind(
                item.price_book_id
                    .unwrap_or_else(|| current.get("price_book_id")),
            )
            .bind(
                item.parent_product_id
                    .unwrap_or_else(|| current.get("parent_product_id")),
            )
            .bind(
                item.parent_variant_id
                    .or_else(|| current.get("parent_variant_id")),
            )
            .bind(
                item.child_product_id
                    .or_else(|| current.get("child_product_id")),
            )
            .bind(
                item.child_variant_id
                    .or_else(|| current.get("child_variant_id")),
            )
            .bind(item.child_label.or_else(|| current.get("child_label")))
            .bind(
                item.relation_type
                    .unwrap_or_else(|| current.get("relation_type")),
            )
            .bind(quantity)
            .bind(conditions)
            .bind(item.notes.or_else(|| current.get("notes")))
            .bind(relation_id)
            .execute(&mut tx)
            .await?;
            updated.push(relation_id);
        } else {
            let price_book_id = item.price_book_id.context("price_book_id is required")?;
            let parent_product_id = item
                .parent_product_id
                .context("parent_product_id is required")?;
            let relation_type = required_string(item.relation_type, "relation_type")?;
            if item.child_product_id.is_none()
                && item.child_variant_id.is_none()
                && item.child_label.as_deref().is_none_or(str::is_empty)
            {
                bail!("a child_product_id, child_variant_id, or child_label is required");
            }
            let quantity = json_text(item.quantity.unwrap_or_else(|| json!({})))?;
            let conditions = json_text(item.conditions.unwrap_or_else(|| json!({})))?;
            let relation_id = db::query(
                "INSERT INTO product_relations \
                    (price_book_id, parent_product_id, parent_variant_id, child_product_id, \
                     child_variant_id, child_label, relation_type, quantity, conditions, notes) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(price_book_id)
            .bind(parent_product_id)
            .bind(item.parent_variant_id)
            .bind(item.child_product_id)
            .bind(item.child_variant_id)
            .bind(item.child_label)
            .bind(relation_type)
            .bind(quantity)
            .bind(conditions)
            .bind(item.notes)
            .execute(&mut tx)
            .await?
            .last_insert_rowid();
            created.push(relation_id);
        }
    }

    let results = tx.commit().await?;
    let created = resolve_ids(created, &results)?;
    Ok(WriteResult {
        created,
        updated,
        deleted: Vec::new(),
    })
}

pub async fn save_pricing_rules(
    store: &PriceBookStore,
    request: SavePricingRulesRequest,
) -> Result<WriteResult> {
    ensure_batch(&request.items)?;
    ensure_unique_ids(request.items.iter().map(|item| item.rule_id))?;
    let mut tx = store.write_batch();
    let mut created = Vec::new();
    let mut updated = Vec::new();

    for item in request.items {
        if let Some(rule_id) = item.rule_id {
            let current = db::query(
                "SELECT price_book_id, product_id, variant_id, category_id, code, name, rule_type, \
                        pricing_mode, json(scope) AS scope_json, \
                        json(conditions) AS conditions_json, json(data) AS data_json, \
                        description, priority, valid_from, valid_to \
                 FROM pricing_rules WHERE id = ?",
            )
            .bind(rule_id)
            .fetch_optional(&tx)
            .await?
            .with_context(|| format!("pricing rule {rule_id} was not found"))?;
            let category_id: Option<i64> = match item.category_code {
                Some(code) => Some(category_id(&mut tx, &code).await?),
                None => current.get("category_id"),
            };
            let scope = match item.scope {
                Some(value) => json_text(value)?,
                None => current.get("scope_json"),
            };
            let conditions = match item.conditions {
                Some(value) => json_text(value)?,
                None => current.get("conditions_json"),
            };
            let data = match item.data {
                Some(value) => json_text(value)?,
                None => current.get("data_json"),
            };
            db::query(
                "UPDATE pricing_rules SET price_book_id = ?, product_id = ?, variant_id = ?, \
                    category_id = ?, code = ?, name = ?, rule_type = ?, pricing_mode = ?, \
                    scope = ?, conditions = ?, data = ?, description = ?, \
                    priority = ?, valid_from = ?, valid_to = ? WHERE id = ?",
            )
            .bind(
                item.price_book_id
                    .unwrap_or_else(|| current.get("price_book_id")),
            )
            .bind(item.product_id.or_else(|| current.get("product_id")))
            .bind(item.variant_id.or_else(|| current.get("variant_id")))
            .bind(category_id)
            .bind(item.code.or_else(|| current.get("code")))
            .bind(item.name.unwrap_or_else(|| current.get("name")))
            .bind(item.rule_type.unwrap_or_else(|| current.get("rule_type")))
            .bind(item.pricing_mode.or_else(|| current.get("pricing_mode")))
            .bind(scope)
            .bind(conditions)
            .bind(data)
            .bind(item.description.or_else(|| current.get("description")))
            .bind(item.priority.unwrap_or_else(|| current.get("priority")))
            .bind(item.valid_from.or_else(|| current.get("valid_from")))
            .bind(item.valid_to.or_else(|| current.get("valid_to")))
            .bind(rule_id)
            .execute(&mut tx)
            .await?;
            updated.push(rule_id);
        } else {
            let price_book_id = item.price_book_id.context("price_book_id is required")?;
            let name = required_string(item.name, "name")?;
            let rule_type = required_string(item.rule_type, "rule_type")?;
            let category_id = match item.category_code {
                Some(code) => Some(category_id(&mut tx, &code).await?),
                None => None,
            };
            let scope = json_text(item.scope.unwrap_or_else(|| json!({})))?;
            let conditions = json_text(item.conditions.unwrap_or_else(|| json!({})))?;
            let data = json_text(item.data.unwrap_or_else(|| json!({})))?;
            let rule_id = db::query(
                "INSERT INTO pricing_rules \
                    (price_book_id, product_id, variant_id, category_id, code, name, rule_type, \
                     pricing_mode, scope, conditions, data, description, priority, valid_from, valid_to) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(price_book_id)
            .bind(item.product_id)
            .bind(item.variant_id)
            .bind(category_id)
            .bind(item.code)
            .bind(name)
            .bind(rule_type)
            .bind(item.pricing_mode)
            .bind(scope)
            .bind(conditions)
            .bind(data)
            .bind(item.description)
            .bind(item.priority.unwrap_or(100))
            .bind(item.valid_from)
            .bind(item.valid_to)
            .execute(&mut tx)
            .await?
            .last_insert_rowid();
            created.push(rule_id);
        }
    }

    let results = tx.commit().await?;
    let created = resolve_ids(created, &results)?;
    Ok(WriteResult {
        created,
        updated,
        deleted: Vec::new(),
    })
}

pub async fn save_documents(
    store: &PriceBookStore,
    request: SaveDocumentsRequest,
) -> Result<WriteResult> {
    ensure_batch(&request.items)?;
    ensure_unique_ids(request.items.iter().map(|item| item.document_id))?;
    let mut tx = store.write_batch();
    let mut created = Vec::new();
    let mut updated = Vec::new();

    for item in request.items {
        if let Some(document_id) = item.document_id {
            let current = db::query(
                "SELECT price_book_id, category_id, doc_type, title, content, \
                        json(data) AS data_json, sort_order \
                 FROM documents WHERE id = ?",
            )
            .bind(document_id)
            .fetch_optional(&tx)
            .await?
            .with_context(|| format!("document {document_id} was not found"))?;
            let category_id: Option<i64> = match item.category_code {
                Some(code) => Some(category_id(&mut tx, &code).await?),
                None => current.get("category_id"),
            };
            let data = match item.data {
                Some(value) => json_text(value)?,
                None => current.get("data_json"),
            };
            db::query(
                "UPDATE documents SET price_book_id = ?, category_id = ?, doc_type = ?, title = ?, \
                    content = ?, data = ?, sort_order = ? WHERE id = ?",
            )
            .bind(
                item.price_book_id
                    .unwrap_or_else(|| current.get("price_book_id")),
            )
            .bind(category_id)
            .bind(item.doc_type.unwrap_or_else(|| current.get("doc_type")))
            .bind(item.title.unwrap_or_else(|| current.get("title")))
            .bind(item.content.or_else(|| current.get("content")))
            .bind(data)
            .bind(item.sort_order.unwrap_or_else(|| current.get("sort_order")))
            .bind(document_id)
            .execute(&mut tx)
            .await?;
            updated.push(document_id);
        } else {
            let doc_type = required_string(item.doc_type, "doc_type")?;
            let title = required_string(item.title, "title")?;
            let category_id = match item.category_code {
                Some(code) => Some(category_id(&mut tx, &code).await?),
                None => None,
            };
            let data = json_text(item.data.unwrap_or_else(|| json!({})))?;
            let document_id = db::query(
                "INSERT INTO documents \
                    (price_book_id, category_id, doc_type, title, content, data, sort_order) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(item.price_book_id)
            .bind(category_id)
            .bind(doc_type)
            .bind(title)
            .bind(item.content)
            .bind(data)
            .bind(item.sort_order.unwrap_or(0))
            .execute(&mut tx)
            .await?
            .last_insert_rowid();
            created.push(document_id);
        }
    }

    let results = tx.commit().await?;
    let created = resolve_ids(created, &results)?;
    Ok(WriteResult {
        created,
        updated,
        deleted: Vec::new(),
    })
}

pub async fn delete_products(
    store: &PriceBookStore,
    request: DeleteProductsRequest,
) -> Result<WriteResult> {
    if request.items.is_empty() {
        bail!("items must not be empty");
    }
    ensure_unique_ids(request.items.iter().map(|item| Some(item.product_id)))?;
    let mut tx = store.write_batch();
    let mut updated = Vec::new();
    let mut deleted = Vec::new();
    for item in request.items {
        match item.mode.as_str() {
            "retire" => {
                db::query("SELECT id FROM products WHERE id = ?")
                    .bind(item.product_id)
                    .fetch_optional(&tx)
                    .await?
                    .with_context(|| format!("product {} was not found", item.product_id))?;
                db::query("UPDATE products SET status = 'discontinued' WHERE id = ?")
                    .bind(item.product_id)
                    .execute(&mut tx)
                    .await?;
                db::query(
                    "UPDATE product_variants SET status = 'discontinued' WHERE product_id = ?",
                )
                .bind(item.product_id)
                .execute(&mut tx)
                .await?;
                updated.push(item.product_id);
            }
            "hard" => {
                db::query("SELECT id FROM products WHERE id = ?")
                    .bind(item.product_id)
                    .fetch_optional(&tx)
                    .await?
                    .with_context(|| format!("product {} was not found", item.product_id))?;
                db::query("DELETE FROM products WHERE id = ?")
                    .bind(item.product_id)
                    .execute(&mut tx)
                    .await?;
                deleted.push(item.product_id);
            }
            other => bail!("invalid delete mode '{other}', expected retire or hard"),
        }
    }
    tx.commit().await?;
    Ok(WriteResult {
        created: Vec::new(),
        updated,
        deleted,
    })
}

pub async fn delete_records(
    store: &PriceBookStore,
    request: DeleteRecordsRequest,
) -> Result<WriteResult> {
    if request.groups.is_empty() {
        bail!("groups must not be empty");
    }
    let mut tx = store.write_batch();
    let mut pending = Vec::new();
    for group in request.groups {
        if group.ids.is_empty() {
            continue;
        }
        let sql = match group.table {
            DeleteTable::Variants => "DELETE FROM product_variants WHERE id = ?",
            DeleteTable::Codes => "DELETE FROM product_codes WHERE id = ?",
            DeleteTable::Prices => "DELETE FROM prices WHERE id = ?",
            DeleteTable::Relations => "DELETE FROM product_relations WHERE id = ?",
            DeleteTable::PricingRules => "DELETE FROM pricing_rules WHERE id = ?",
            DeleteTable::Documents => "DELETE FROM documents WHERE id = ?",
        };
        for id in group.ids {
            let write = db::query(sql).bind(id).execute(&mut tx).await?;
            pending.push((id, write));
        }
    }
    let results = tx.commit().await?;
    let mut deleted = Vec::new();
    for (id, write) in pending {
        if write.rows_affected(&results)? > 0 {
            deleted.push(id);
        }
    }
    Ok(WriteResult {
        created: Vec::new(),
        updated: Vec::new(),
        deleted,
    })
}

async fn update_product(
    tx: &mut WriteBatch,
    product_id: i64,
    item: crate::write_models::ProductWrite,
) -> Result<()> {
    let current = db::query(
        "SELECT category_id, product_type, name, description, json(properties) AS properties_json, \
                status, valid_from, valid_to FROM products WHERE id = ?",
    )
    .bind(product_id)
    .fetch_optional(&*tx)
    .await?
    .with_context(|| format!("product {product_id} was not found"))?;

    let category_id = match item.category_code {
        Some(code) => category_id(tx, &code).await?,
        None => current.get("category_id"),
    };
    let name = match item.name {
        Some(value) => required_nonempty(value, "name")?,
        None => current.get("name"),
    };
    let properties = match item.properties {
        Some(value) => json_text(value)?,
        None => current.get("properties_json"),
    };
    db::query(
        "UPDATE products SET category_id = ?, product_type = ?, name = ?, description = ?, \
            properties = ?, status = ?, valid_from = ?, valid_to = ? WHERE id = ?",
    )
    .bind(category_id)
    .bind(
        item.product_type
            .unwrap_or_else(|| current.get("product_type")),
    )
    .bind(name)
    .bind(item.description.or_else(|| current.get("description")))
    .bind(properties)
    .bind(item.status.unwrap_or_else(|| current.get("status")))
    .bind(item.valid_from.or_else(|| current.get("valid_from")))
    .bind(item.valid_to.or_else(|| current.get("valid_to")))
    .bind(product_id)
    .execute(&mut *tx)
    .await?;

    if let Some(scopes) = item.scopes {
        if scopes.is_empty() {
            bail!("scopes cannot be empty when replacing product scopes");
        }
        replace_scopes(tx, product_id, scopes).await?;
    }
    Ok(())
}

async fn insert_new_product_scopes(
    tx: &mut WriteBatch,
    product_id: i64,
    scopes: Vec<crate::write_models::ScopeWrite>,
) -> Result<()> {
    let mut query =
        db::QueryBuilder::new("INSERT INTO product_scopes (product_id, series, product_domain) ");
    for (index, scope) in scopes.into_iter().enumerate() {
        if index > 0 {
            query.push(" UNION ALL ");
        }
        let series = required_nonempty(scope.series, "series")?;
        let domain = required_nonempty(scope.domain, "domain")?;
        query
            .push("SELECT ")
            .push_bind(product_id)
            .push(", ")
            .push_bind(series)
            .push(", ")
            .push_bind(domain);
    }
    let statement = query.build();
    if statement.binding_count() > 100 {
        bail!("product scopes exceed D1's 100 bound-parameter limit");
    }
    statement.execute(&mut *tx).await?;
    Ok(())
}

async fn replace_scopes(
    tx: &mut WriteBatch,
    product_id: i64,
    scopes: Vec<crate::write_models::ScopeWrite>,
) -> Result<()> {
    db::query("DELETE FROM product_scopes WHERE product_id = ?")
        .bind(product_id)
        .execute(&mut *tx)
        .await?;
    for scope in scopes {
        let series = required_nonempty(scope.series, "series")?;
        let domain = required_nonempty(scope.domain, "domain")?;
        db::query(
            "INSERT INTO product_scopes (product_id, series, product_domain) VALUES (?, ?, ?)",
        )
        .bind(product_id)
        .bind(series)
        .bind(domain)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

async fn category_id(tx: &mut WriteBatch, code: &str) -> Result<i64> {
    let code = code.trim();
    let row = db::query("SELECT id FROM categories WHERE code = ?")
        .bind(code)
        .fetch_optional(&*tx)
        .await?
        .with_context(|| format!("unknown category code '{code}'"))?;
    Ok(row.get("id"))
}

async fn validate_code_variant(
    tx: &mut WriteBatch,
    product_id: i64,
    variant_id: Option<i64>,
) -> Result<()> {
    if let Some(variant_id) = variant_id {
        let row = db::query("SELECT product_id FROM product_variants WHERE id = ?")
            .bind(variant_id)
            .fetch_optional(&*tx)
            .await?
            .with_context(|| format!("variant {variant_id} was not found"))?;
        let actual_product: i64 = row.get("product_id");
        if actual_product != product_id {
            bail!("variant {variant_id} does not belong to product {product_id}");
        }
    }
    Ok(())
}

async fn validate_rule_book(tx: &mut WriteBatch, rule_id: i64, price_book_id: i64) -> Result<()> {
    let row = db::query("SELECT price_book_id FROM pricing_rules WHERE id = ?")
        .bind(rule_id)
        .fetch_optional(&*tx)
        .await?
        .with_context(|| format!("pricing rule {rule_id} was not found"))?;
    let actual_book: i64 = row.get("price_book_id");
    if actual_book != price_book_id {
        bail!("pricing rule {rule_id} belongs to a different price book");
    }
    Ok(())
}

fn validate_price_amount(amount: Option<&str>) -> Result<()> {
    if let Some(amount) = amount {
        let amount = amount.trim();
        if amount.is_empty() {
            bail!("amount_decimal must not be empty");
        }
        let mut dot = false;
        let mut digits = 0;
        for (index, character) in amount.chars().enumerate() {
            match character {
                '-' if index == 0 => {}
                '.' if !dot => dot = true,
                '0'..='9' => digits += 1,
                _ => bail!("amount_decimal must be a plain decimal string"),
            }
        }
        if digits == 0 {
            bail!("amount_decimal must contain digits");
        }
    }
    Ok(())
}

fn ensure_unique_ids(ids: impl IntoIterator<Item = Option<i64>>) -> Result<()> {
    let mut seen = HashSet::new();
    for id in ids.into_iter().flatten() {
        if !seen.insert(id) {
            bail!("ID {id} appears more than once in one D1 write batch");
        }
    }
    Ok(())
}

fn ensure_batch<T>(items: &[T]) -> Result<()> {
    if items.is_empty() {
        bail!("items must not be empty");
    }
    if items.len() > 500 {
        bail!("at most 500 items are allowed per call");
    }
    Ok(())
}

fn required_string(value: Option<String>, field: &str) -> Result<String> {
    value
        .with_context(|| format!("{field} is required"))
        .and_then(|value| required_nonempty(value, field))
}

fn required_nonempty(value: String, field: &str) -> Result<String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(value)
}

fn json_text(value: Value) -> Result<String> {
    serde_json::to_string(&value).context("failed to serialize JSON value")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_plain_decimal_strings() {
        for valid in ["0", "12.34", "-15.00", ".5", "1."] {
            assert!(validate_price_amount(Some(valid)).is_ok(), "{valid}");
        }
        for invalid in ["", " ", "-", ".", "1.2.3", "+1", "1e3", "abc"] {
            assert!(validate_price_amount(Some(invalid)).is_err(), "{invalid}");
        }
        assert!(validate_price_amount(None).is_ok());
    }

    #[test]
    fn validates_required_strings_and_batch_sizes() {
        assert_eq!(
            required_string(Some("  name  ".to_owned()), "name").unwrap(),
            "name"
        );
        assert!(required_string(None, "name").is_err());
        assert!(required_nonempty("  ".to_owned(), "name").is_err());
        assert!(ensure_batch(&[1]).is_ok());
        assert!(ensure_batch::<i32>(&[]).is_err());
        assert!(ensure_batch(&vec![0; 501]).is_err());
        assert!(ensure_unique_ids([None, None, Some(1), Some(2)]).is_ok());
        assert!(ensure_unique_ids([Some(1), Some(1)]).is_err());
    }

    #[test]
    fn serializes_json_as_text() {
        assert_eq!(
            json_text(json!({"color": "灰色"})).unwrap(),
            r#"{"color":"灰色"}"#
        );
    }
}
