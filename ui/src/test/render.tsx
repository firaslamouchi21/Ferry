import type { ReactElement } from "react";
import { render } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { FerryClient, MockTransport } from "@/lib/ipc";
import { FerryProvider } from "@/lib/query";
import { I18nProvider } from "@/lib/i18n";
import { ThemeProvider } from "@/lib/theme";

export function renderScreen(node: ReactElement, options?: { route?: string; client?: FerryClient }) {
  const client = options?.client ?? new FerryClient("mock", new MockTransport());
  const utils = render(
    <ThemeProvider>
      <I18nProvider locale="en">
        <MemoryRouter initialEntries={[options?.route ?? "/"]}>
          <FerryProvider client={client}>{node}</FerryProvider>
        </MemoryRouter>
      </I18nProvider>
    </ThemeProvider>,
  );
  return { ...utils, client };
}
