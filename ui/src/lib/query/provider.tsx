import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { FerryClient, type ConnectionPhase, type IpcEvent } from "@/lib/ipc";
import { queryKeys, resourceInvalidations } from "./keys";

export interface TransferProgress {
  bytes: number;
  total: number;
}

interface FerryContextValue {
  client: FerryClient;
  phase: ConnectionPhase;
  lastEvent: IpcEvent | null;
}

const FerryContext = createContext<FerryContextValue | null>(null);

export function useFerry(): FerryContextValue {
  const value = useContext(FerryContext);
  if (!value) throw new Error("useFerry must be used inside <FerryProvider>");
  return value;
}

export function useFerryClient(): FerryClient {
  return useFerry().client;
}

export function FerryProvider({ children, client }: { children: ReactNode; client?: FerryClient }) {
  const queryClient = useMemo(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: { retry: 1, staleTime: 2_000, refetchOnWindowFocus: false },
        },
      }),
    [],
  );
  const ferryRef = useRef(client ?? new FerryClient());
  const ferry = ferryRef.current;
  const [phase, setPhase] = useState<ConnectionPhase>(ferry.phase());
  const [lastEvent, setLastEvent] = useState<IpcEvent | null>(null);

  useEffect(() => {
    const offPhase = ferry.onPhase((next) => {
      setPhase(next);
      if (next === "connected") {
        void queryClient.invalidateQueries();
      }
    });
    const offEvent = ferry.onEvent((event) => {
      setLastEvent(event);
      if (event.event === "changed") {
        for (const key of resourceInvalidations[event.params.resource]) {
          void queryClient.invalidateQueries({ queryKey: key });
        }
      } else if (event.event === "progress") {
        const bytes = typeof event.params.bytes === "bigint" ? Number(event.params.bytes) : event.params.bytes;
        const total = typeof event.params.total === "bigint" ? Number(event.params.total) : event.params.total;
        queryClient.setQueryData<TransferProgress>(queryKeys.progress(event.params.item_id), { bytes, total });
      }
    });
    ferry.start();
    return () => {
      offPhase();
      offEvent();
      ferry.stop();
    };
  }, [ferry, queryClient]);

  const value = useMemo<FerryContextValue>(() => ({ client: ferry, phase, lastEvent }), [ferry, phase, lastEvent]);

  return (
    <QueryClientProvider client={queryClient}>
      <FerryContext.Provider value={value}>{children}</FerryContext.Provider>
    </QueryClientProvider>
  );
}
