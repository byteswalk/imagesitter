import { createRouter, createWebHashHistory } from "vue-router";

// 使用 hash 路由：Tauri 自定义协议下刷新与深链最稳妥
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      name: "capture",
      component: () => import("@/views/CaptureView.vue"),
      meta: { title: "捕获" },
    },
    {
      path: "/objects",
      name: "objects",
      component: () => import("@/views/ObjectsView.vue"),
      meta: { title: "对象与特征" },
    },
    {
      path: "/calibrate",
      name: "calibrate",
      component: () => import("@/views/CalibrateView.vue"),
      meta: { title: "校准" },
    },
    {
      path: "/replay",
      name: "replay",
      component: () => import("@/views/ReplayView.vue"),
      meta: { title: "回放测试" },
    },
    {
      path: "/export",
      name: "export",
      component: () => import("@/views/ExportView.vue"),
      meta: { title: "导出" },
    },
    {
      path: "/settings",
      name: "settings",
      component: () => import("@/views/SettingsView.vue"),
      meta: { title: "设置" },
    },
  ],
});

export default router;
