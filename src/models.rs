use std::collections::HashMap;

use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListPriceBooksRequest {
    /// Include draft and retired price books. Active books are returned by default.
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListPriceBooksResponse {
    pub price_books: Vec<PriceBookSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PriceBookSummary {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub series: String,
    pub domain: String,
    pub version_label: String,
    pub currency: String,
    pub status: String,
    pub effective_from: Option<String>,
    pub effective_to: Option<String>,
    pub price_count: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchProductsRequest {
    /// Batch of independent catalogue searches. Prefer one call containing all
    /// products/components recognized from the design drawing.
    pub queries: Vec<SearchQuery>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchQuery {
    /// Free text matched against product names and product codes.
    pub text: Option<String>,
    /// Optional exact/prefix code candidates, e.g. ["WJ0006-A", "SGF-A1001"].
    #[serde(default)]
    pub codes: Vec<String>,
    /// Category code such as cabinet_and_door, door, hinge, drawer, countertop.
    pub category: Option<String>,
    /// Series: J, P, or S.
    pub series: Option<String>,
    /// Product domain: kitchen or home.
    pub domain: Option<String>,
    /// Maximum candidates for this query. Defaults to 8, capped at 25.
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchProductsResponse {
    pub results: Vec<SearchResultGroup>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResultGroup {
    pub index: usize,
    pub matches: Vec<ProductSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProductSummary {
    pub product_id: i64,
    pub name: String,
    pub category_code: String,
    pub category_name: String,
    pub product_type: String,
    pub status: String,
    pub series: Vec<String>,
    pub primary_codes: Vec<String>,
    pub variant_count: i64,
    pub sample_variants: Vec<VariantSummary>,
    pub match_score: i64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct VariantSummary {
    pub variant_id: i64,
    pub variant_key: String,
    pub label: Option<String>,
    pub properties: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchColorsRequest {
    /// Batch of color names/codes recognized from the design drawing.
    pub queries: Vec<ColorQuery>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ColorQuery {
    /// Color name, code, or alias, e.g. "珍珠白哑光", "SP02001".
    pub text: String,
    /// Maximum candidates for this query. Defaults to 8, capped at 20.
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchColorsResponse {
    pub results: Vec<ColorResultGroup>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ColorResultGroup {
    pub index: usize,
    pub matches: Vec<ColorSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ColorSummary {
    pub color_id: i64,
    pub code: String,
    pub name: String,
    pub color_class: Option<String>,
    pub material: Option<String>,
    pub status: String,
    pub properties: Value,
    pub aliases: Vec<String>,
    pub match_score: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetQuoteDataRequest {
    /// Series: J, P, or S. The matching active price book is selected.
    pub series: String,
    /// Product domain: kitchen or home.
    pub domain: String,
    /// Batch of selections resolved together. Use product_id after search or a
    /// code when the code is already known.
    pub selections: Vec<ProductSelection>,
    /// Maximum variants returned per product. Defaults to 100, capped at 300.
    pub max_variants_per_product: Option<usize>,
    /// Include applicable pricing rules and documents. Defaults to true.
    pub include_pricing_context: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductSelection {
    /// Product ID from search_products or an earlier response.
    pub product_id: Option<i64>,
    /// Exact product code. If ambiguous, candidates are returned instead of a
    /// detail record.
    pub code: Option<String>,
    /// Exact/range filters applied to variant properties. Example:
    /// {"width_mm": 600, "height_mm": 720, "direction": "left"}.
    /// Range form: {"height_mm": {"min": 700, "max": 800}}.
    #[serde(default)]
    pub variant_filters: HashMap<String, Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GetQuoteDataResponse {
    pub price_book: PriceBookSummary,
    pub items: Vec<QuoteDataItem>,
    pub pricing_rules: Vec<PricingRuleView>,
    pub documents: Vec<DocumentView>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct QuoteDataItem {
    pub selection_index: usize,
    pub product: Option<ProductDetail>,
    /// Populated when a code maps to multiple products.
    pub candidates: Vec<ProductSummary>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProductDetail {
    pub product_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub category_code: String,
    pub category_name: String,
    pub product_type: String,
    pub status: String,
    pub series: Vec<String>,
    pub codes: Vec<CodeView>,
    pub properties: Value,
    pub variants: Vec<VariantDetail>,
    pub truncated_variants: bool,
    pub relations: Vec<RelationView>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CodeView {
    pub code: String,
    pub code_type: String,
    pub variant_id: Option<i64>,
    pub is_primary: bool,
    pub conditions: Value,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct VariantDetail {
    pub variant_id: i64,
    pub variant_key: String,
    pub label: Option<String>,
    pub status: String,
    pub properties: Value,
    pub constraints: Value,
    pub prices: Vec<PriceView>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PriceView {
    pub id: i64,
    pub price_kind: String,
    pub amount_decimal: Option<String>,
    pub currency: String,
    pub unit: String,
    pub unit_label: Option<String>,
    pub measurement_basis: String,
    pub pricing_mode: Option<String>,
    pub conditions: Value,
    pub derivation_rule_id: Option<i64>,
    pub notes: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RelationView {
    pub id: i64,
    pub relation_type: String,
    pub parent_product_id: i64,
    pub parent_name: String,
    pub parent_codes: Vec<String>,
    pub parent_variant_id: Option<i64>,
    pub child_product_id: Option<i64>,
    pub child_name: Option<String>,
    pub child_codes: Vec<String>,
    pub child_variant_id: Option<i64>,
    pub child_label: Option<String>,
    pub quantity: Value,
    pub conditions: Value,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PricingRuleView {
    pub id: i64,
    pub code: Option<String>,
    pub name: String,
    pub rule_type: String,
    pub pricing_mode: Option<String>,
    pub scope: Value,
    pub conditions: Value,
    pub data: Value,
    pub description: Option<String>,
    pub priority: i64,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentView {
    pub id: i64,
    pub doc_type: String,
    pub title: String,
    pub content: Option<String>,
    pub data: Value,
    pub sort_order: i64,
}
