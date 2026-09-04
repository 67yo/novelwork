import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import { bootUiTheme } from "./lib/uiTheme";
import "./style.css";

bootUiTheme();
createApp(App).use(router).mount("#app");
