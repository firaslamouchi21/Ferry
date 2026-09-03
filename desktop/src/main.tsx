import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter } from "react-router-dom";
import { App } from "@ferry/ui/App";
import { FerryProvider } from "@ferry/ui/lib/query";
import { FerryClient } from "@ferry/ui/lib/ipc";
import { TauriTransport } from "./TauriTransport";
import "@ferry/ui/styles/base.css";
import "@ferry/ui/styles/app.css";

const client = new FerryClient("browser", new TauriTransport());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <HashRouter>
      <FerryProvider client={client}>
        <App />
      </FerryProvider>
    </HashRouter>
  </StrictMode>,
);
