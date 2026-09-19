import { BrowserRouter } from "react-router-dom";

import { createTauriHostBridge } from "../bridge";
import type { HostBridge } from "../bridge";
import { AppRouter } from "./router";

const defaultBridge = createTauriHostBridge();

export interface AppProps {
  bridge?: HostBridge;
}

export function App({ bridge = defaultBridge }: AppProps = {}) {
  return (
    <BrowserRouter>
      <AppRouter bridge={bridge} />
    </BrowserRouter>
  );
}
