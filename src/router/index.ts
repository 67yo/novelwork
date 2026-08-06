import { createRouter, createWebHistory } from "vue-router";
import LibraryView from "@/views/LibraryView.vue";
import NovelsView from "@/views/NovelsView.vue";
import WorkspaceView from "@/views/WorkspaceView.vue";
import StatsView from "@/views/StatsView.vue";
import SettingsView from "@/views/SettingsView.vue";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", redirect: "/novels" },
    { path: "/library", name: "library", component: LibraryView },
    { path: "/novels", name: "novels", component: NovelsView },
    { path: "/novels/:id", name: "workspace", component: WorkspaceView, props: true },
    { path: "/stats", name: "stats", component: StatsView },
    { path: "/settings", name: "settings", component: SettingsView },
  ],
});

export default router;
