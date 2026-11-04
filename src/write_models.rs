use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, JsonSchema)]
pub struct WriteResult {
    pub created: Vec<i64>,
    pub updated: Vec<i64>,
    pub deleted: Vec<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScopeWrite {
    /// Series: J, P, or S.
    pub series: String,
    /// Product domain: kitchen or home.
    pub domain: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveProductsRequest {
    pub items: Vec<ProductWrite>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProductWrite {
    /// Existing product ID for update. Omit to create.
    pub product_id: Option<i64>,
    /// Required when creating. Category code from the categories table.
    pub category_code: Option<String>,
    pub product_type: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    /// Descriptive properties. Replaces the whole JSON object when supplied.
    pub properties: Option<Value>,
    pub status: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    /// Required when creating. Supplying this on update replaces all scopes.
    pub scopes: Option<Vec<ScopeWrite>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveVariantsRequest {
    pub items: Vec<VariantWrite>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VariantWrite {
    /// Existing variant ID for update. Omit to create.
    pub variant_id: Option<i64>,
    /// Required when creating.
    pub product_id: Option<i64>,
    /// Stable key unique within the product. Required when creating.
    pub variant_key: Option<String>,
    pub label: Option<String>,
    /// Physical attributes. Replaces the whole JSON object when supplied.
    pub properties: Option<Value>,
    /// Production constraints. Replaces the whole JSON object when supplied.
    pub constraints: Option<Value>,
    pub status: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveCodesRequest {
    pub items: Vec<CodeWrite>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CodeWrite {
    /// Existing code row ID for update. Omit to create.
    pub code_id: Option<i64>,
    /// Required when creating.
    pub product_id: Option<i64>,
    pub variant_id: Option<i64>,
    /// Required when creating.
    pub code: Option<String>,
    pub code_type: Option<String>,
    pub is_primary: Option<bool>,
    /// Conditions under which this alias applies. Replaces the object when supplied.
    pub conditions: Option<Value>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SavePricesRequest {
    pub items: Vec<PriceWrite>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PriceWrite {
    /// Existing price ID for update. Omit to create.
    pub price_id: Option<i64>,
    /// Required when creating.
    pub price_book_id: Option<i64>,
    /// Required when creating.
    pub variant_id: Option<i64>,
    pub price_kind: Option<String>,
    /// Exact decimal string, e.g. "648.198645134165". Keep null when using a
    /// derived price rule.
    pub amount_decimal: Option<String>,
    pub currency: Option<String>,
    pub unit: Option<String>,
    pub unit_label: Option<String>,
    pub measurement_basis: Option<String>,
    pub pricing_mode: Option<String>,
    /// Price applicability conditions. Replaces the object when supplied.
    pub conditions: Option<Value>,
    pub derivation_rule_id: Option<i64>,
    pub notes: Option<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveRelationsRequest {
    pub items: Vec<RelationWrite>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RelationWrite {
    /// Existing relation ID for update. Omit to create.
    pub relation_id: Option<i64>,
    pub price_book_id: Option<i64>,
    pub parent_product_id: Option<i64>,
    pub parent_variant_id: Option<i64>,
    pub child_product_id: Option<i64>,
    pub child_variant_id: Option<i64>,
    pub child_label: Option<String>,
    pub relation_type: Option<String>,
    pub quantity: Option<Value>,
    pub conditions: Option<Value>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SavePricingRulesRequest {
    pub items: Vec<PricingRuleWrite>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PricingRuleWrite {
    /// Existing rule ID for update. Omit to create.
    pub rule_id: Option<i64>,
    pub price_book_id: Option<i64>,
    pub product_id: Option<i64>,
    pub variant_id: Option<i64>,
    pub category_code: Option<String>,
    pub code: Option<String>,
    pub name: Option<String>,
    pub rule_type: Option<String>,
    pub pricing_mode: Option<String>,
    pub scope: Option<Value>,
    pub conditions: Option<Value>,
    pub data: Option<Value>,
    pub description: Option<String>,
    pub priority: Option<i64>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveDocumentsRequest {
    pub items: Vec<DocumentWrite>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DocumentWrite {
    /// Existing document ID for update. Omit to create.
    pub document_id: Option<i64>,
    pub price_book_id: Option<i64>,
    pub category_code: Option<String>,
    pub doc_type: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    pub data: Option<Value>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteProductsRequest {
    pub items: Vec<DeleteProductItem>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteProductItem {
    pub product_id: i64,
    /// `retire` changes status to discontinued; `hard` permanently deletes the
    /// product and cascades to variants, codes and prices.
    pub mode: String,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeleteTable {
    Variants,
    Codes,
    Prices,
    Relations,
    PricingRules,
    Documents,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteRecordsRequest {
    pub groups: Vec<DeleteGroup>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteGroup {
    pub table: DeleteTable,
    pub ids: Vec<i64>,
}
