import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app/App";
import "./styles/tokens.css";
import "./styles/reset.css";
import "./styles/layout.css";

const rootElement = document.getElementById("root");
if (!rootElement) {
  throw new Error("找不到桌面 UI 根节点");
}

createRoot(rootElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
