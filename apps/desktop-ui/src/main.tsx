import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";

import { App } from "./app/App";
import { createQueryClient } from "./data/queryClient";
import "./styles/tokens.css";
import "./styles/reset.css";
import "./styles/layout.css";

const rootElement = document.getElementById("root");
if (!rootElement) {
  throw new Error("找不到桌面 UI 根节点");
}

const queryClient = createQueryClient();

createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
);
