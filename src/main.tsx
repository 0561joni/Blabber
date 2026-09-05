import { ShutdownBoundary } from "./components/ShutdownBoundary";
import { AppearanceProvider } from "./lib/appearance";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppearanceProvider>
      <ShutdownBoundary>
        <App />
      </ShutdownBoundary>
    </AppearanceProvider>
  </StrictMode>,
);
