import { sveltekit } from "@sveltejs/kit/vite";

const config = {
  plugins: [sveltekit()],

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
