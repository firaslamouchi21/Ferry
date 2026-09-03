import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter } from "react-router-dom";
import { FerryProvider } from "@/lib/query";
import { I18nProvider } from "@/lib/i18n";
import { App } from "./App";
import "./styles/base.css";
import "./styles/app.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <I18nProvider>
      <HashRouter>
        <FerryProvider>
          <App />
        </FerryProvider>
      </HashRouter>
    </I18nProvider>
  </StrictMode>,
);
