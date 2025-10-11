// vite.config.mts
import { defineConfig } from "file:///Users/a123/DevelopmentFiles/orderReceiving/cc-switch/node_modules/.pnpm/vite@5.4.19_@types+node@20.19.9_lightningcss@1.30.1/node_modules/vite/dist/node/index.js";
import react from "file:///Users/a123/DevelopmentFiles/orderReceiving/cc-switch/node_modules/.pnpm/@vitejs+plugin-react@4.7.0_vite@5.4.19_@types+node@20.19.9_lightningcss@1.30.1_/node_modules/@vitejs/plugin-react/dist/index.js";
import tailwindcss from "file:///Users/a123/DevelopmentFiles/orderReceiving/cc-switch/node_modules/.pnpm/@tailwindcss+vite@4.1.13_vite@5.4.19_@types+node@20.19.9_lightningcss@1.30.1_/node_modules/@tailwindcss/vite/dist/index.mjs";
var vite_config_default = defineConfig({
  root: "src",
  plugins: [react(), tailwindcss()],
  base: "./",
  build: {
    outDir: "../dist",
    emptyOutDir: true
  },
  server: {
    port: 3e3,
    strictPort: true
  },
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"]
});
export {
  vite_config_default as default
};
//# sourceMappingURL=data:application/json;base64,ewogICJ2ZXJzaW9uIjogMywKICAic291cmNlcyI6IFsidml0ZS5jb25maWcubXRzIl0sCiAgInNvdXJjZXNDb250ZW50IjogWyJjb25zdCBfX3ZpdGVfaW5qZWN0ZWRfb3JpZ2luYWxfZGlybmFtZSA9IFwiL1VzZXJzL2ExMjMvRGV2ZWxvcG1lbnRGaWxlcy9vcmRlclJlY2VpdmluZy9jYy1zd2l0Y2hcIjtjb25zdCBfX3ZpdGVfaW5qZWN0ZWRfb3JpZ2luYWxfZmlsZW5hbWUgPSBcIi9Vc2Vycy9hMTIzL0RldmVsb3BtZW50RmlsZXMvb3JkZXJSZWNlaXZpbmcvY2Mtc3dpdGNoL3ZpdGUuY29uZmlnLm10c1wiO2NvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9pbXBvcnRfbWV0YV91cmwgPSBcImZpbGU6Ly8vVXNlcnMvYTEyMy9EZXZlbG9wbWVudEZpbGVzL29yZGVyUmVjZWl2aW5nL2NjLXN3aXRjaC92aXRlLmNvbmZpZy5tdHNcIjtpbXBvcnQgeyBkZWZpbmVDb25maWcgfSBmcm9tIFwidml0ZVwiO1xuaW1wb3J0IHJlYWN0IGZyb20gXCJAdml0ZWpzL3BsdWdpbi1yZWFjdFwiO1xuaW1wb3J0IHRhaWx3aW5kY3NzIGZyb20gXCJAdGFpbHdpbmRjc3Mvdml0ZVwiO1xuXG5leHBvcnQgZGVmYXVsdCBkZWZpbmVDb25maWcoe1xuICByb290OiBcInNyY1wiLFxuICBwbHVnaW5zOiBbcmVhY3QoKSwgdGFpbHdpbmRjc3MoKV0sXG4gIGJhc2U6IFwiLi9cIixcbiAgYnVpbGQ6IHtcbiAgICBvdXREaXI6IFwiLi4vZGlzdFwiLFxuICAgIGVtcHR5T3V0RGlyOiB0cnVlLFxuICB9LFxuICBzZXJ2ZXI6IHtcbiAgICBwb3J0OiAzMDAwLFxuICAgIHN0cmljdFBvcnQ6IHRydWUsXG4gIH0sXG4gIGNsZWFyU2NyZWVuOiBmYWxzZSxcbiAgZW52UHJlZml4OiBbXCJWSVRFX1wiLCBcIlRBVVJJX1wiXSxcbn0pO1xuIl0sCiAgIm1hcHBpbmdzIjogIjtBQUFtVixTQUFTLG9CQUFvQjtBQUNoWCxPQUFPLFdBQVc7QUFDbEIsT0FBTyxpQkFBaUI7QUFFeEIsSUFBTyxzQkFBUSxhQUFhO0FBQUEsRUFDMUIsTUFBTTtBQUFBLEVBQ04sU0FBUyxDQUFDLE1BQU0sR0FBRyxZQUFZLENBQUM7QUFBQSxFQUNoQyxNQUFNO0FBQUEsRUFDTixPQUFPO0FBQUEsSUFDTCxRQUFRO0FBQUEsSUFDUixhQUFhO0FBQUEsRUFDZjtBQUFBLEVBQ0EsUUFBUTtBQUFBLElBQ04sTUFBTTtBQUFBLElBQ04sWUFBWTtBQUFBLEVBQ2Q7QUFBQSxFQUNBLGFBQWE7QUFBQSxFQUNiLFdBQVcsQ0FBQyxTQUFTLFFBQVE7QUFDL0IsQ0FBQzsiLAogICJuYW1lcyI6IFtdCn0K
