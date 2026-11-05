import { defineWranglerConfig } from "wrangler/experimental-config";

export default defineWranglerConfig({
	build: {
		command: "cargo install -q --git https://github.com/cloudflare/workers-rs --rev 606f27fd93efc698f575ab7e84c6be66e5535468 worker-build && worker-build --emscripten --release",
	},
});
