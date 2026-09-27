import { ShutdownBoundary } from "./components/ShutdownBoundary";
import { AppearanceProvider } from "./lib/appearance";
import { createRoot } from "react-dom/client";
import { SplashApp } from "./SplashApp";
import "./splash.css";

createRoot(document.getElementById("root")!).render(
  <AppearanceProvider>
    <ShutdownBoundary>
      <SplashApp />
    </ShutdownBoundary>
  </AppearanceProvider>,
);
