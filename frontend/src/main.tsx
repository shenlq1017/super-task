import { StrictMode, Component, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider, createHashRouter } from "react-router-dom";
import "./i18n"; // i18next 初始化（先于 App 渲染）
import { App } from "./app/App";
import "./index.css";

// Graceful fallback: surface render errors on screen instead of a blank window.
class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  render() {
    if (this.state.error) {
      return (
        <pre
          style={{
            margin: 24,
            padding: 16,
            background: "#1e1e1e",
            color: "#ff9b9b",
            font: "12px/1.5 monospace",
            whiteSpace: "pre-wrap",
          }}
        >
          {"渲染出错：\n" + (this.state.error.stack ?? this.state.error.message)}
        </pre>
      );
    }
    return this.props.children;
  }
}

// data router（v7）：route JSX 保持不变（App 内 <Routes>），useBlocker 等 blocker 能力依赖 data router。
const router = createHashRouter([{ path: "*", element: <App /> }]);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <RouterProvider router={router} />
    </ErrorBoundary>
  </StrictMode>,
);
