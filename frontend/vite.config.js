import { fileURLToPath } from "node:url";
import adapter from "@sveltejs/adapter-node";
import { sveltekit } from "@sveltejs/kit/vite";

const config = {
  plugins: [sveltekit({ adapter: adapter() })],
  resolve: { alias: { $lib: fileURLToPath(new URL("./src/lib", import.meta.url)) } },

  server: {
    // Proxy /api requests to the backend during local development.
    // This allows using relative URLs ("/api/...") everywhere,
    // matching the production Nginx behaviour.
    proxy: {
      "/api": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
    },
  },
};

export default config;
