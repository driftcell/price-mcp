use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{Json, ServerHandler, tool, tool_handler, tool_router};

use crate::models::{
    GetQuoteDataRequest, GetQuoteDataResponse, ListPriceBooksRequest, ListPriceBooksResponse,
    SearchColorsRequest, SearchColorsResponse, SearchProductsRequest, SearchProductsResponse,
};
use crate::store::PriceBookStore;
use crate::write_models::{
    DeleteProductsRequest, DeleteRecordsRequest, SaveCodesRequest, SaveDocumentsRequest,
    SavePricesRequest, SavePricingRulesRequest, SaveProductsRequest, SaveRelationsRequest,
    SaveVariantsRequest, WriteResult,
};
use crate::write_store;

#[derive(Clone)]
pub struct PriceBookServer {
    store: PriceBookStore,
    #[expect(dead_code, reason = "tool_handler macro reads this router")]
    tool_router: ToolRouter<Self>,
}

impl PriceBookServer {
    pub fn new(store: PriceBookStore) -> Self {
        Self {
            store,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl PriceBookServer {
    #[tool(
        description = "List available price books and their active status. Use this when the series/domain scope is unclear."
    )]
    async fn list_price_books(
        &self,
        Parameters(request): Parameters<ListPriceBooksRequest>,
    ) -> Result<Json<ListPriceBooksResponse>, String> {
        self.store
            .list_price_books(request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Batch-search products by design-drawing terms or codes. Send all recognized product/component queries in one call. Matches include product IDs, category, series, codes and sample variants for the next get_quote_data call."
    )]
    async fn search_products(
        &self,
        Parameters(request): Parameters<SearchProductsRequest>,
    ) -> Result<Json<SearchProductsResponse>, String> {
        self.store
            .search_products(request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Batch-resolve color names or codes from a design drawing to color IDs, color classes, material, status and aliases. Use this before selecting door/panel prices whose conditions depend on color_class."
    )]
    async fn search_colors(
        &self,
        Parameters(request): Parameters<SearchColorsRequest>,
    ) -> Result<Json<SearchColorsResponse>, String> {
        self.store
            .search_colors(request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Batch-fetch everything needed to reason about a quote: selected products, matching variants, all factory/retail prices and conditions, applicable pricing rules, relations and documents. Supply dimensions through variant_filters. This does not select a price or calculate a quote; the Agent must interpret the returned conditions and rules."
    )]
    async fn get_quote_data(
        &self,
        Parameters(request): Parameters<GetQuoteDataRequest>,
    ) -> Result<Json<GetQuoteDataResponse>, String> {
        self.store
            .get_quote_data(request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Create or update a batch of products. Creating requires category_code, name and at least one series/domain scope. Supplying scopes on update replaces all existing scopes.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn save_products(
        &self,
        Parameters(request): Parameters<SaveProductsRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::save_products(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Create or update a batch of product variants. Variant keys must be unique within a product.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn save_variants(
        &self,
        Parameters(request): Parameters<SaveVariantsRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::save_variants(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Create or update a batch of product code aliases. A variant referenced by a code must belong to the same product.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn save_product_codes(
        &self,
        Parameters(request): Parameters<SaveCodesRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::save_codes(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Create or update a batch of price rows. Prices store exact decimal strings and separate factory/retail kinds. A price must have amount_decimal or a derivation pricing rule.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn save_prices(
        &self,
        Parameters(request): Parameters<SavePricesRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::save_prices(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Create or update a batch of product relations such as included, required, optional, excluded, compatible_with and replaced_by.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn save_relations(
        &self,
        Parameters(request): Parameters<SaveRelationsRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::save_relations(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Create or update a batch of pricing rules, coefficients, tier definitions and pricing notes.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn save_pricing_rules(
        &self,
        Parameters(request): Parameters<SavePricingRulesRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::save_pricing_rules(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Create or update a batch of process, policy, changelog, installation or pricing note documents.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn save_documents(
        &self,
        Parameters(request): Parameters<SaveDocumentsRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::save_documents(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Retire products (status discontinued and variants discontinued) or permanently delete products in a batch. Hard deletion cascades to variants, codes, prices and relations.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn delete_products(
        &self,
        Parameters(request): Parameters<DeleteProductsRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::delete_products(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }

    #[tool(
        description = "Permanently delete rows from variants, codes, prices, relations, pricing rules or documents by ID. Use delete_products for product-level retirement or deletion.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn delete_records(
        &self,
        Parameters(request): Parameters<DeleteRecordsRequest>,
    ) -> Result<Json<WriteResult>, String> {
        write_store::delete_records(&self.store, request)
            .await
            .map(Json)
            .map_err(|error| format!("{error:#}"))
    }
}

#[tool_handler(
    name = "pricebook-mcp",
    version = "0.1.0",
    instructions = "报价检索工具。读取客户设计图后，先一次性批量调用 search_products，将所有识别到的产品和组件作为 queries 提交；图纸中的花色名称/色号用 search_colors 批量解析。再根据返回的 product_id 一次性批量调用 get_quote_data。尺寸通过 variant_filters 精确过滤。get_quote_data 返回所有价格条件、非标规则、包含关系和文档，但不会自动选价或计算报价，必须由 Agent 根据图纸和返回条件判断。若产品状态为 discontinued 或发现缺失价格/条件，必须在最终报价中明确提示。"
)]
impl ServerHandler for PriceBookServer {}
