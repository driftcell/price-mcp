use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use worker::d1::D1Database;

use crate::db::{DbRow, QueryBuilder, Row, query};
use crate::models::{
    CodeView, ColorResultGroup, ColorSummary, DocumentView, GetQuoteDataRequest,
    GetQuoteDataResponse, ListPriceBooksRequest, ListPriceBooksResponse, PriceBookSummary,
    PriceView, PricingRuleView, ProductDetail, ProductSelection, ProductSummary, QuoteDataItem,
    RelationView, SearchColorsRequest, SearchColorsResponse, SearchProductsRequest,
    SearchProductsResponse, SearchQuery, SearchResultGroup, VariantDetail, VariantSummary,
};

#[derive(Clone)]
pub struct PriceBookStore {
    database: Arc<D1Database>,
}

struct ProductBase {
    product_id: i64,
    name: String,
    description: Option<String>,
    category_code: String,
    category_name: String,
    product_type: String,
    status: String,
    properties: Value,
}

impl PriceBookStore {
    pub fn new(database: Arc<D1Database>) -> Self {
        Self { database }
    }

    pub(crate) fn write_batch(&self) -> crate::db::WriteBatch {
        crate::db::WriteBatch::new(Arc::clone(&self.database))
    }

    pub async fn list_price_books(
        &self,
        request: ListPriceBooksRequest,
    ) -> Result<ListPriceBooksResponse> {
        let sql = if request.include_inactive {
            "SELECT pb.*, \
                (SELECT COUNT(*) FROM prices p WHERE p.price_book_id = pb.id) AS price_count \
             FROM price_books pb ORDER BY pb.series, pb.product_domain, pb.id"
        } else {
            "SELECT pb.*, \
                (SELECT COUNT(*) FROM prices p WHERE p.price_book_id = pb.id) AS price_count \
             FROM price_books pb WHERE pb.status = 'active' \
             ORDER BY pb.series, pb.product_domain, pb.id"
        };

        let rows = query(sql).fetch_all(&self.database).await?;
        let mut price_books = Vec::with_capacity(rows.len());
        for row in rows {
            price_books.push(price_book_from_row(&row)?);
        }
        Ok(ListPriceBooksResponse { price_books })
    }

    pub async fn search_products(
        &self,
        request: SearchProductsRequest,
    ) -> Result<SearchProductsResponse> {
        if request.queries.is_empty() {
            bail!("queries must not be empty");
        }
        if request.queries.len() > 100 {
            bail!("at most 100 search queries are allowed per call");
        }

        let mut results = Vec::with_capacity(request.queries.len());
        let mut warnings = Vec::new();
        for (index, query) in request.queries.into_iter().enumerate() {
            if query.text.as_deref().is_none_or(str::is_empty) && query.codes.is_empty() {
                warnings.push(format!(
                    "query {index}: text and codes are both empty; only structural filters were used"
                ));
            }
            let matches = self.search_one(&query).await?;
            if matches.is_empty() {
                warnings.push(format!("query {index}: no products matched"));
            }
            results.push(SearchResultGroup { index, matches });
        }
        Ok(SearchProductsResponse { results, warnings })
    }

    pub async fn search_colors(
        &self,
        request: SearchColorsRequest,
    ) -> Result<SearchColorsResponse> {
        if request.queries.is_empty() {
            bail!("queries must not be empty");
        }
        if request.queries.len() > 100 {
            bail!("at most 100 color queries are allowed per call");
        }

        let mut results = Vec::with_capacity(request.queries.len());
        let mut warnings = Vec::new();
        for (index, query) in request.queries.into_iter().enumerate() {
            let text = query.text.trim();
            if text.is_empty() {
                warnings.push(format!("color query {index}: text is empty"));
                results.push(ColorResultGroup {
                    index,
                    matches: Vec::new(),
                });
                continue;
            }
            let limit = query.limit.unwrap_or(8).clamp(1, 20);
            let pattern = format!("%{text}%");
            let text_upper = text.to_uppercase();
            let rows = crate::db::query(
                "SELECT c.id, c.code, c.name, c.color_class, c.material, c.status, \
                        json(c.properties) AS properties_json, \
                        COALESCE((SELECT group_concat(ca.alias, '||') FROM color_aliases ca \
                                  WHERE ca.color_id = c.id), '') AS aliases \
                 FROM colors c \
                 WHERE c.name LIKE ? COLLATE NOCASE OR c.code LIKE ? COLLATE NOCASE \
                    OR EXISTS (SELECT 1 FROM color_aliases ca2 \
                               WHERE ca2.color_id = c.id AND ca2.alias LIKE ? COLLATE NOCASE) \
                 ORDER BY c.name, c.code LIMIT 250",
            )
            .bind(&pattern)
            .bind(&pattern)
            .bind(&pattern)
            .fetch_all(&self.database)
            .await?;

            let mut matches = rows
                .into_iter()
                .map(|row| {
                    let code: String = row.get("code");
                    let name: String = row.get("name");
                    let aliases = split_aliases(row.get("aliases"));
                    let score = color_score(&text_upper, &code, &name, &aliases);
                    Ok(ColorSummary {
                        color_id: row.get("id"),
                        code,
                        name,
                        color_class: row.get("color_class"),
                        material: row.get("material"),
                        status: row.get("status"),
                        properties: parse_json(row.get("properties_json"))?,
                        aliases,
                        match_score: score,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            matches.sort_by(|a, b| {
                b.match_score
                    .cmp(&a.match_score)
                    .then_with(|| a.name.cmp(&b.name))
            });
            matches.truncate(limit);
            if matches.is_empty() {
                warnings.push(format!("color query {index}: no colors matched"));
            }
            results.push(ColorResultGroup { index, matches });
        }

        Ok(SearchColorsResponse { results, warnings })
    }

    async fn search_one(&self, query: &SearchQuery) -> Result<Vec<ProductSummary>> {
        let limit = query.limit.unwrap_or(8).clamp(1, 25);
        let fetch_limit = (limit * 10).clamp(limit, 250);
        let mut qb = QueryBuilder::new(
            "SELECT p.id, p.name, c.code AS category_code, c.name AS category_name, \
                    p.product_type, p.status, \
                    COALESCE( \
                        (SELECT group_concat(DISTINCT pc.code) FROM product_codes pc \
                         WHERE pc.product_id = p.id AND pc.is_primary = 1), \
                        (SELECT group_concat(DISTINCT pc.code) FROM product_codes pc \
                         WHERE pc.product_id = p.id) \
                    ) AS primary_codes, \
                    (SELECT group_concat(DISTINCT pc.code) FROM product_codes pc \
                     WHERE pc.product_id = p.id) AS all_codes, \
                    (SELECT group_concat(DISTINCT ps.series) FROM product_scopes ps \
                     WHERE ps.product_id = p.id) AS series, \
                    (SELECT COUNT(*) FROM product_variants v WHERE v.product_id = p.id) \
                        AS variant_count \
             FROM products p JOIN categories c ON c.id = p.category_id WHERE 1 = 1",
        );

        if let Some(text) = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let pattern = format!("%{text}%");
            qb.push(" AND (p.name LIKE ").push_bind(pattern.clone());
            qb.push(
                " COLLATE NOCASE OR EXISTS (SELECT 1 FROM product_codes pcx \
                     WHERE pcx.product_id = p.id AND pcx.code LIKE ",
            );
            qb.push_bind(pattern);
            qb.push(" COLLATE NOCASE))");
        }

        if !query.codes.is_empty() {
            qb.push(" AND (");
            for (i, code) in query
                .codes
                .iter()
                .filter(|c| !c.trim().is_empty())
                .enumerate()
            {
                if i > 0 {
                    qb.push(" OR ");
                }
                qb.push(
                    "EXISTS (SELECT 1 FROM product_codes pcc \
                         WHERE pcc.product_id = p.id AND pcc.code = ",
                );
                qb.push_bind(code.trim());
                qb.push(")");
            }
            qb.push(")");
        }

        if let Some(category) = query
            .category
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            qb.push(" AND c.code = ").push_bind(category);
        }
        if let Some(series) = query
            .series
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            qb.push(
                " AND EXISTS (SELECT 1 FROM product_scopes pss \
                     WHERE pss.product_id = p.id AND pss.series = ",
            )
            .push_bind(series);
            qb.push(")");
        }
        if let Some(domain) = query
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            qb.push(
                " AND EXISTS (SELECT 1 FROM product_scopes psd \
                     WHERE psd.product_id = p.id AND psd.product_domain = ",
            )
            .push_bind(domain);
            qb.push(")");
        }

        qb.push(" ORDER BY p.name LIMIT ")
            .push_bind(fetch_limit as i64);
        let statement = qb.build();
        if statement.binding_count() > 100 {
            bail!("one product search exceeds D1's 100 bound-parameter limit");
        }
        let rows = statement.fetch_all(&self.database).await?;

        let text_upper = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_uppercase);
        let wanted_codes: Vec<String> = query
            .codes
            .iter()
            .map(|c| c.trim().to_uppercase())
            .filter(|c| !c.is_empty())
            .collect();

        let mut summaries = Vec::with_capacity(rows.len());
        for row in rows {
            let primary_codes = split_codes(row.get::<Option<String>, _>("primary_codes"));
            let all_codes = split_codes(row.get::<Option<String>, _>("all_codes"));
            let score = search_score(
                text_upper.as_deref(),
                &wanted_codes,
                &row.get::<String, _>("name"),
                &all_codes,
            );
            summaries.push(ProductSummary {
                product_id: row.get("id"),
                name: row.get("name"),
                category_code: row.get("category_code"),
                category_name: row.get("category_name"),
                product_type: row.get("product_type"),
                status: row.get("status"),
                series: split_codes(row.get::<Option<String>, _>("series")),
                primary_codes,
                variant_count: row.get("variant_count"),
                sample_variants: Vec::new(),
                match_score: score,
            });
        }

        summaries.sort_by(|a, b| {
            b.match_score
                .cmp(&a.match_score)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.product_id.cmp(&b.product_id))
        });
        summaries.truncate(limit);

        for summary in &mut summaries {
            summary.sample_variants = self.sample_variants(summary.product_id).await?;
        }
        Ok(summaries)
    }

    async fn sample_variants(&self, product_id: i64) -> Result<Vec<VariantSummary>> {
        let rows = query(
            "SELECT id, variant_key, label, json(properties) AS properties_json \
             FROM product_variants WHERE product_id = ? ORDER BY id LIMIT 3",
        )
        .bind(product_id)
        .fetch_all(&self.database)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(VariantSummary {
                    variant_id: row.get("id"),
                    variant_key: row.get("variant_key"),
                    label: row.get("label"),
                    properties: parse_json(row.get("properties_json"))?,
                })
            })
            .collect()
    }

    pub async fn get_quote_data(
        &self,
        request: GetQuoteDataRequest,
    ) -> Result<GetQuoteDataResponse> {
        if request.selections.is_empty() {
            bail!("selections must not be empty");
        }
        if request.selections.len() > 100 {
            bail!("at most 100 product selections are allowed per call");
        }
        let series = request.series.trim().to_uppercase();
        let domain = request.domain.trim().to_lowercase();
        if !matches!(domain.as_str(), "kitchen" | "home") {
            bail!("domain must be kitchen or home");
        }

        let price_book = self.active_price_book(&series, &domain).await?;
        let max_variants = request
            .max_variants_per_product
            .unwrap_or(100)
            .clamp(1, 300);
        let include_context = request.include_pricing_context.unwrap_or(true);

        let mut items = Vec::with_capacity(request.selections.len());
        let mut warnings = Vec::new();
        let mut resolved_ids = HashSet::new();
        let mut category_ids = HashSet::new();
        let mut derivation_rule_ids = HashSet::new();

        for (index, selection) in request.selections.into_iter().enumerate() {
            match self
                .resolve_selection(&price_book, index, selection, max_variants)
                .await
            {
                Ok(item) => {
                    if let Some(product) = &item.product {
                        resolved_ids.insert(product.product_id);
                        category_ids.insert(
                            self.product_category_id(product.product_id)
                                .await
                                .unwrap_or_default(),
                        );
                        for variant in &product.variants {
                            for price in &variant.prices {
                                if let Some(rule_id) = price.derivation_rule_id {
                                    derivation_rule_ids.insert(rule_id);
                                }
                            }
                        }
                    }
                    if let Some(error) = &item.error {
                        warnings.push(format!("selection {index}: {error}"));
                    }
                    items.push(item);
                }
                Err(error) => {
                    warnings.push(format!("selection {index}: {error:#}"));
                    items.push(QuoteDataItem {
                        selection_index: index,
                        product: None,
                        candidates: Vec::new(),
                        error: Some(format!("{error:#}")),
                    });
                }
            }
        }

        let relations = self.load_relations(price_book.id, &resolved_ids).await?;
        for item in &mut items {
            if let Some(product) = &mut item.product {
                product.relations = relations
                    .iter()
                    .filter(|relation| {
                        relation.parent_product_id == product.product_id
                            || relation.child_product_id == Some(product.product_id)
                    })
                    .cloned()
                    .collect();
            }
        }

        let mut pricing_rules = Vec::new();
        let mut documents = Vec::new();
        if include_context {
            category_ids.remove(&0);
            pricing_rules = self
                .load_rules(
                    price_book.id,
                    &resolved_ids,
                    &category_ids,
                    &derivation_rule_ids,
                )
                .await?;
            documents = self.load_documents(price_book.id, &category_ids).await?;
        }

        for item in &items {
            if let Some(product) = &item.product {
                if product.variants.is_empty() {
                    warnings.push(format!(
                        "selection {}: no variants matched the supplied filters",
                        item.selection_index
                    ));
                } else if product
                    .variants
                    .iter()
                    .all(|variant| variant.prices.is_empty())
                {
                    warnings.push(format!(
                        "selection {}: product '{}' has no prices in price book '{}'",
                        item.selection_index, product.name, price_book.slug
                    ));
                }
                if product.truncated_variants {
                    warnings.push(format!(
                        "selection {}: variants were truncated; add variant_filters or raise max_variants_per_product",
                        item.selection_index
                    ));
                }
            }
        }

        Ok(GetQuoteDataResponse {
            price_book,
            items,
            pricing_rules,
            documents,
            warnings,
        })
    }

    async fn resolve_selection(
        &self,
        price_book: &PriceBookSummary,
        index: usize,
        selection: ProductSelection,
        max_variants: usize,
    ) -> Result<QuoteDataItem> {
        let (product_id, candidates) = match (selection.product_id, selection.code) {
            (Some(id), None) => (Some(id), Vec::new()),
            (None, Some(code)) if !code.trim().is_empty() => {
                let rows = query(
                    "SELECT DISTINCT product_id FROM product_codes \
                     WHERE code = ? COLLATE NOCASE ORDER BY product_id",
                )
                .bind(code.trim())
                .fetch_all(&self.database)
                .await?;
                let ids: Vec<i64> = rows.into_iter().map(|row| row.get("product_id")).collect();
                match ids.as_slice() {
                    [] => {
                        return Ok(QuoteDataItem {
                            selection_index: index,
                            product: None,
                            candidates: Vec::new(),
                            error: Some(format!("code '{}' was not found", code.trim())),
                        });
                    }
                    [id] => (Some(*id), Vec::new()),
                    _ => {
                        let mut candidates = Vec::with_capacity(ids.len());
                        for id in ids {
                            candidates.push(self.product_summary(id).await?);
                        }
                        return Ok(QuoteDataItem {
                            selection_index: index,
                            product: None,
                            candidates,
                            error: Some(format!(
                                "code '{}' maps to multiple products; choose a product_id",
                                code.trim()
                            )),
                        });
                    }
                }
            }
            (None, _) => {
                return Ok(QuoteDataItem {
                    selection_index: index,
                    product: None,
                    candidates: Vec::new(),
                    error: Some("provide exactly one of product_id or code".to_owned()),
                });
            }
            (Some(_), Some(_)) => {
                return Ok(QuoteDataItem {
                    selection_index: index,
                    product: None,
                    candidates: Vec::new(),
                    error: Some("provide only one of product_id or code".to_owned()),
                });
            }
        };

        let Some(product_id) = product_id else {
            return Ok(QuoteDataItem {
                selection_index: index,
                product: None,
                candidates: Vec::new(),
                error: Some("product could not be resolved".to_owned()),
            });
        };

        let product = self
            .load_product_detail(
                price_book,
                product_id,
                max_variants,
                &selection.variant_filters,
            )
            .await?;
        Ok(QuoteDataItem {
            selection_index: index,
            product: Some(product),
            candidates,
            error: None,
        })
    }

    async fn active_price_book(&self, series: &str, domain: &str) -> Result<PriceBookSummary> {
        let rows = query(
            "SELECT pb.*, (SELECT COUNT(*) FROM prices p WHERE p.price_book_id = pb.id) AS price_count \
             FROM price_books pb \
             WHERE pb.status = 'active' AND pb.series = ? AND pb.product_domain = ? \
             ORDER BY pb.id",
        )
        .bind(series)
        .bind(domain)
        .fetch_all(&self.database)
        .await?;

        match rows.len() {
            0 => bail!("no active price book for series={series}, domain={domain}"),
            1 => price_book_from_row(&rows[0]),
            _ => {
                let slugs: Vec<String> = rows.iter().map(|row| row.get("slug")).collect();
                bail!(
                    "multiple active price books for series={series}, domain={domain}: {}",
                    slugs.join(", ")
                )
            }
        }
    }

    async fn product_category_id(&self, product_id: i64) -> Result<i64> {
        let query = query("SELECT category_id FROM products WHERE id = ?").bind(product_id);
        Ok(query.fetch_one(&self.database).await?.get("category_id"))
    }

    async fn product_summary(&self, product_id: i64) -> Result<ProductSummary> {
        let row = query(
            "SELECT p.id, p.name, c.code AS category_code, c.name AS category_name, \
                    p.product_type, p.status, \
                    COALESCE( \
                        (SELECT group_concat(DISTINCT pc.code) FROM product_codes pc \
                         WHERE pc.product_id = p.id AND pc.is_primary = 1), \
                        (SELECT group_concat(DISTINCT pc.code) FROM product_codes pc \
                         WHERE pc.product_id = p.id) \
                    ) AS primary_codes, \
                    (SELECT group_concat(DISTINCT ps.series) FROM product_scopes ps \
                     WHERE ps.product_id = p.id) AS series, \
                    (SELECT COUNT(*) FROM product_variants v WHERE v.product_id = p.id) \
                        AS variant_count \
             FROM products p JOIN categories c ON c.id = p.category_id \
             WHERE p.id = ?",
        )
        .bind(product_id)
        .fetch_one(&self.database)
        .await?;

        Ok(ProductSummary {
            product_id,
            name: row.get("name"),
            category_code: row.get("category_code"),
            category_name: row.get("category_name"),
            product_type: row.get("product_type"),
            status: row.get("status"),
            series: split_codes(row.get::<Option<String>, _>("series")),
            primary_codes: split_codes(row.get::<Option<String>, _>("primary_codes")),
            variant_count: row.get("variant_count"),
            sample_variants: self.sample_variants(product_id).await?,
            match_score: 0,
        })
    }

    async fn load_product_detail(
        &self,
        price_book: &PriceBookSummary,
        product_id: i64,
        max_variants: usize,
        filters: &HashMap<String, Value>,
    ) -> Result<ProductDetail> {
        let row = query(
            "SELECT p.id, p.name, p.description, p.category_id, \
                    c.code AS category_code, c.name AS category_name, \
                    p.product_type, p.status, json(p.properties) AS properties_json \
             FROM products p JOIN categories c ON c.id = p.category_id WHERE p.id = ?",
        )
        .bind(product_id)
        .fetch_optional(&self.database)
        .await?
        .with_context(|| format!("product {product_id} was not found"))?;

        let base = ProductBase {
            product_id: row.get("id"),
            name: row.get("name"),
            description: row.get("description"),
            category_code: row.get("category_code"),
            category_name: row.get("category_name"),
            product_type: row.get("product_type"),
            status: row.get("status"),
            properties: parse_json(row.get("properties_json"))?,
        };

        let scale_rows = query(
            "SELECT group_concat(DISTINCT series) AS series FROM product_scopes WHERE product_id = ?",
        )
        .bind(product_id)
        .fetch_one(&self.database)
        .await?;
        let series = split_codes(scale_rows.get::<Option<String>, _>("series"));

        let code_rows = query(
            "SELECT code, code_type, variant_id, is_primary, json(conditions) AS conditions_json, notes \
             FROM product_codes WHERE product_id = ? ORDER BY is_primary DESC, id",
        )
        .bind(product_id)
        .fetch_all(&self.database)
        .await?;
        let codes = code_rows
            .into_iter()
            .map(|row| {
                Ok(CodeView {
                    code: row.get("code"),
                    code_type: row.get("code_type"),
                    variant_id: row.get("variant_id"),
                    is_primary: row.get::<i64, _>("is_primary") != 0,
                    conditions: parse_json(row.get("conditions_json"))?,
                    notes: row.get("notes"),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let price_rows = query(
            "SELECT pr.id, pr.variant_id, pr.price_kind, pr.amount_decimal, pr.currency, \
                    pr.unit, pr.unit_label, pr.measurement_basis, pr.pricing_mode, \
                    json(pr.conditions) AS conditions_json, pr.derivation_rule_id, pr.notes, \
                    pr.valid_from, pr.valid_to \
             FROM prices pr JOIN product_variants v ON v.id = pr.variant_id \
             WHERE v.product_id = ? AND pr.price_book_id = ? \
             ORDER BY pr.variant_id, pr.price_kind, pr.id",
        )
        .bind(product_id)
        .bind(price_book.id)
        .fetch_all(&self.database)
        .await?;
        let mut prices_by_variant: HashMap<i64, Vec<PriceView>> = HashMap::new();
        for row in price_rows {
            let variant_id: i64 = row.get("variant_id");
            prices_by_variant
                .entry(variant_id)
                .or_default()
                .push(PriceView {
                    id: row.get("id"),
                    price_kind: row.get("price_kind"),
                    amount_decimal: row.get("amount_decimal"),
                    currency: row.get("currency"),
                    unit: row.get("unit"),
                    unit_label: row.get("unit_label"),
                    measurement_basis: row.get("measurement_basis"),
                    pricing_mode: row.get("pricing_mode"),
                    conditions: parse_json(row.get("conditions_json"))?,
                    derivation_rule_id: row.get("derivation_rule_id"),
                    notes: row.get("notes"),
                    valid_from: row.get("valid_from"),
                    valid_to: row.get("valid_to"),
                });
        }

        let variant_rows = query(
            "SELECT id, variant_key, label, status, json(properties) AS properties_json, \
                    json(constraints) AS constraints_json \
             FROM product_variants WHERE product_id = ? ORDER BY id",
        )
        .bind(product_id)
        .fetch_all(&self.database)
        .await?;

        let mut matched_variants = Vec::new();
        let mut total_matched = 0usize;
        for row in variant_rows {
            let properties = parse_json(row.get("properties_json"))?;
            if !variant_matches(&properties, filters) {
                continue;
            }
            total_matched += 1;
            if matched_variants.len() >= max_variants {
                continue;
            }
            let variant_id: i64 = row.get("id");
            matched_variants.push(VariantDetail {
                variant_id,
                variant_key: row.get("variant_key"),
                label: row.get("label"),
                status: row.get("status"),
                properties,
                constraints: parse_json(row.get("constraints_json"))?,
                prices: prices_by_variant.remove(&variant_id).unwrap_or_default(),
            });
        }

        Ok(ProductDetail {
            product_id: base.product_id,
            name: base.name,
            description: base.description,
            category_code: base.category_code,
            category_name: base.category_name,
            product_type: base.product_type,
            status: base.status,
            series,
            codes,
            properties: base.properties,
            variants: matched_variants,
            truncated_variants: total_matched > max_variants,
            relations: Vec::new(),
        })
    }

    async fn load_relations(
        &self,
        price_book_id: i64,
        selected_ids: &HashSet<i64>,
    ) -> Result<Vec<RelationView>> {
        if selected_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = query(
            "SELECT r.id, r.relation_type, r.parent_product_id, pp.name AS parent_name, \
                    (SELECT group_concat(DISTINCT pc.code) FROM product_codes pc \
                     WHERE pc.product_id = r.parent_product_id) AS parent_codes, \
                    r.parent_variant_id, r.child_product_id, cp.name AS child_name, \
                    (SELECT group_concat(DISTINCT pc.code) FROM product_codes pc \
                     WHERE pc.product_id = r.child_product_id) AS child_codes, \
                    r.child_variant_id, r.child_label, json(r.quantity) AS quantity_json, \
                    json(r.conditions) AS conditions_json, r.notes \
             FROM product_relations r \
             JOIN products pp ON pp.id = r.parent_product_id \
             LEFT JOIN products cp ON cp.id = r.child_product_id \
             WHERE r.price_book_id = ? ORDER BY r.id",
        )
        .bind(price_book_id)
        .fetch_all(&self.database)
        .await?;

        rows.into_iter()
            .filter(|row| {
                let parent_id: i64 = row.get("parent_product_id");
                let child_id: Option<i64> = row.get("child_product_id");
                selected_ids.contains(&parent_id)
                    || child_id.is_some_and(|id| selected_ids.contains(&id))
            })
            .map(|row| {
                Ok(RelationView {
                    id: row.get("id"),
                    relation_type: row.get("relation_type"),
                    parent_product_id: row.get("parent_product_id"),
                    parent_name: row.get("parent_name"),
                    parent_codes: split_codes(row.get::<Option<String>, _>("parent_codes")),
                    parent_variant_id: row.get("parent_variant_id"),
                    child_product_id: row.get("child_product_id"),
                    child_name: row.get("child_name"),
                    child_codes: split_codes(row.get::<Option<String>, _>("child_codes")),
                    child_variant_id: row.get("child_variant_id"),
                    child_label: row.get("child_label"),
                    quantity: parse_json(row.get("quantity_json"))?,
                    conditions: parse_json(row.get("conditions_json"))?,
                    notes: row.get("notes"),
                })
            })
            .collect()
    }

    async fn load_rules(
        &self,
        price_book_id: i64,
        selected_ids: &HashSet<i64>,
        category_ids: &HashSet<i64>,
        derivation_rule_ids: &HashSet<i64>,
    ) -> Result<Vec<PricingRuleView>> {
        let rows = query(
            "SELECT pr.id, COALESCE(pr.product_id, rv.product_id) AS effective_product_id, \
                    pr.category_id, pr.code, pr.name, pr.rule_type, pr.pricing_mode, \
                    json(pr.scope) AS scope_json, json(pr.conditions) AS conditions_json, \
                    json(pr.data) AS data_json, pr.description, pr.priority, \
                    pr.valid_from, pr.valid_to \
             FROM pricing_rules pr \
             LEFT JOIN product_variants rv ON rv.id = pr.variant_id \
             WHERE pr.price_book_id = ? ORDER BY pr.priority, pr.id",
        )
        .bind(price_book_id)
        .fetch_all(&self.database)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            let rule_id: i64 = row.get("id");
            let product_id: Option<i64> = row.get("effective_product_id");
            let category_id: Option<i64> = row.get("category_id");
            let referenced = derivation_rule_ids.contains(&rule_id);
            let relevant = referenced
                || product_id.is_some_and(|id| selected_ids.contains(&id))
                || (product_id.is_none()
                    && category_id.is_none_or(|id| category_ids.contains(&id)));
            if !relevant {
                continue;
            }
            result.push(PricingRuleView {
                id: rule_id,
                code: row.get("code"),
                name: row.get("name"),
                rule_type: row.get("rule_type"),
                pricing_mode: row.get("pricing_mode"),
                scope: parse_json(row.get("scope_json"))?,
                conditions: parse_json(row.get("conditions_json"))?,
                data: parse_json(row.get("data_json"))?,
                description: row.get("description"),
                priority: row.get("priority"),
                valid_from: row.get("valid_from"),
                valid_to: row.get("valid_to"),
            });
        }
        Ok(result)
    }

    async fn load_documents(
        &self,
        price_book_id: i64,
        category_ids: &HashSet<i64>,
    ) -> Result<Vec<DocumentView>> {
        let rows = query(
            "SELECT id, category_id, doc_type, title, content, json(data) AS data_json, sort_order \
             FROM documents WHERE price_book_id = ? ORDER BY sort_order, id",
        )
        .bind(price_book_id)
        .fetch_all(&self.database)
        .await?;

        rows.into_iter()
            .filter(|row| {
                let category_id: Option<i64> = row.get("category_id");
                category_id.is_none_or(|id| category_ids.contains(&id))
            })
            .map(|row| {
                Ok(DocumentView {
                    id: row.get("id"),
                    doc_type: row.get("doc_type"),
                    title: row.get("title"),
                    content: row.get("content"),
                    data: parse_json(row.get("data_json"))?,
                    sort_order: row.get("sort_order"),
                })
            })
            .collect()
    }
}

fn price_book_from_row(row: &DbRow) -> Result<PriceBookSummary> {
    Ok(PriceBookSummary {
        id: row.get("id"),
        slug: row.get("slug"),
        name: row.get("name"),
        series: row.get("series"),
        domain: row.get("product_domain"),
        version_label: row.get("version_label"),
        currency: row.get("currency"),
        status: row.get("status"),
        effective_from: row.get("effective_from"),
        effective_to: row.get("effective_to"),
        price_count: row.get("price_count"),
    })
}

fn split_codes(value: Option<String>) -> Vec<String> {
    let mut result: Vec<String> = value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    result.sort();
    result.dedup();
    result
}

fn split_aliases(value: String) -> Vec<String> {
    let mut result: Vec<String> = value
        .split("||")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    result.sort();
    result.dedup();
    result
}

fn parse_json(value: String) -> Result<Value> {
    serde_json::from_str(&value).context("failed to parse JSON from D1")
}

fn color_score(text_upper: &str, code: &str, name: &str, aliases: &[String]) -> i64 {
    let code_upper = code.to_uppercase();
    let name_upper = name.to_uppercase();
    let aliases_upper: Vec<String> = aliases.iter().map(|alias| alias.to_uppercase()).collect();

    if code_upper == text_upper {
        1000
    } else if aliases_upper.iter().any(|alias| alias == text_upper) {
        900
    } else if name_upper == text_upper {
        800
    } else if code_upper.contains(text_upper)
        || aliases_upper.iter().any(|a| a.contains(text_upper))
    {
        500
    } else if name_upper.contains(text_upper) {
        400
    } else {
        0
    }
}

fn search_score(
    text_upper: Option<&str>,
    wanted_codes: &[String],
    name: &str,
    all_codes: &[String],
) -> i64 {
    let codes_upper: Vec<String> = all_codes.iter().map(|c| c.to_uppercase()).collect();
    let mut score = 0;

    for code in wanted_codes {
        if codes_upper.iter().any(|candidate| candidate == code) {
            score = score.max(1000);
        } else if codes_upper
            .iter()
            .any(|candidate| candidate.starts_with(code))
        {
            score = score.max(700);
        }
    }

    if let Some(text) = text_upper {
        if name.to_uppercase() == text {
            score = score.max(600);
        } else if name.to_uppercase().contains(text) {
            score = score.max(400);
        }
        if codes_upper.iter().any(|candidate| candidate == text) {
            score = score.max(1000);
        } else if codes_upper
            .iter()
            .any(|candidate| candidate.starts_with(text))
        {
            score = score.max(800);
        } else if codes_upper.iter().any(|candidate| candidate.contains(text)) {
            score = score.max(600);
        }
    }

    score
}

fn variant_matches(properties: &Value, filters: &HashMap<String, Value>) -> bool {
    let Some(object) = properties.as_object() else {
        return filters.is_empty();
    };
    filters.iter().all(|(key, expected)| {
        let Some(actual) = object.get(key) else {
            return false;
        };
        value_matches(actual, expected)
    })
}

fn value_matches(actual: &Value, expected: &Value) -> bool {
    if actual == expected {
        return true;
    }

    let Some(filter) = expected.as_object() else {
        return false;
    };

    if let Some(values) = filter.get("in").and_then(Value::as_array) {
        return values.iter().any(|value| value == actual);
    }
    if let Some(value) = filter.get("equals") {
        return value == actual;
    }

    let actual_number = actual
        .as_f64()
        .or_else(|| actual.as_str().and_then(|value| value.parse().ok()));
    let Some(actual_number) = actual_number else {
        return false;
    };
    let min = filter.get("min").and_then(Value::as_f64);
    let max = filter.get("max").and_then(Value::as_f64);
    let include_min = filter
        .get("include_min")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let include_max = filter
        .get("include_max")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if let Some(min) = min {
        let above = if include_min {
            actual_number >= min
        } else {
            actual_number > min
        };
        if !above {
            return false;
        }
    }
    if let Some(max) = max {
        let below = if include_max {
            actual_number <= max
        } else {
            actual_number < max
        };
        if !below {
            return false;
        }
    }
    min.is_some() || max.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn variant_filters_match_exact_and_ranges() {
        let properties = json!({
            "width_mm": 600,
            "height_mm": 720,
            "direction": "left",
            "color": "灰色"
        });
        let filters = HashMap::from([
            ("width_mm".to_owned(), json!(600)),
            ("direction".to_owned(), json!("left")),
            (
                "height_mm".to_owned(),
                json!({"min": 700, "max": 800, "include_min": true, "include_max": true}),
            ),
            ("color".to_owned(), json!({"in": ["灰色", "白色"]})),
        ]);
        assert!(variant_matches(&properties, &filters));
    }

    #[test]
    fn variant_filters_reject_missing_or_out_of_range() {
        let properties = json!({"height_mm": 810, "direction": "right"});
        assert!(!variant_matches(
            &properties,
            &HashMap::from([("direction".to_owned(), json!("left"))])
        ));
        assert!(!variant_matches(
            &properties,
            &HashMap::from([("height_mm".to_owned(), json!({"max": 800}))])
        ));
        assert!(!variant_matches(
            &properties,
            &HashMap::from([("width_mm".to_owned(), json!(600))])
        ));
    }

    #[test]
    fn color_code_and_alias_rank_before_partial_name() {
        assert_eq!(color_score("SP02001", "SP02001", "珍珠白哑光", &[]), 1000);
        assert_eq!(
            color_score("威特斯", "SP01009", "威斯特", &["威特斯".to_owned()]),
            900
        );
        assert!(color_score("珍珠", "SP02001", "珍珠白哑光", &[]) > 0);
    }

    #[test]
    fn exact_code_ranks_above_name_matches() {
        let codes = vec!["WJ0006-A".to_owned(), "WJ0006-B".to_owned()];
        assert_eq!(
            search_score(Some("WJ0006-A"), &[], "百隆铰链", &codes),
            1000
        );
        assert_eq!(search_score(None, &[], "百隆铰链", &codes), 0);
        assert!(search_score(Some("百隆"), &[], "百隆铰链", &codes) > 0);
    }
}
