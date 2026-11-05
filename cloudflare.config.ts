import { bindings, defineConfig } from "cf/config";

export default defineConfig({
	worker: {
		name: "pricebook-mcp",
		compatibilityDate: "2026-11-04",
		compatibilityFlags: ["new_module_registry"],
		entrypoint: "build/index.js",
		env: {
			MCP_ALLOWED_HOSTS: bindings.text(
				"localhost,127.0.0.1,pricebook-mcp.driftcell.workers.dev",
			),
			MCP_BEARER_TOKEN: bindings.secret(),
			DB: bindings.d1({
				name: "pricebook-mcp-experiment",
				id: "0ffca771-1783-43cb-a0ff-d3e99e874565",
			}),
		},
	},
});
