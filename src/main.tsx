import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyTheme, loadTheme } from "./theme";

// 首次渲染前就应用主题，避免启动时闪一下默认配色。
applyTheme(loadTheme());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
