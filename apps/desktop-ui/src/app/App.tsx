import { BrowserRouter } from "react-router-dom";

import { createTauriHostBridge } from "../bridge";
import type { HostBridge } from "../bridge";
import { applyTheme, readPreference } from "./theme";
import { AppRouter } from "./router";

// 模块作用域应用主题：真实入口与测试 harness 都经过 App，可避免首帧主题闪变。
applyTheme(readPreference());

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
