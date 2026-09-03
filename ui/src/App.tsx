import { Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "@/app/AppShell";
import { PeersScreen } from "@/screens/PeersScreen";
import { SendScreen } from "@/screens/SendScreen";
import { InboxScreen } from "@/screens/InboxScreen";
import { ItemDetailScreen } from "@/screens/ItemDetailScreen";
import { SentScreen } from "@/screens/SentScreen";
import { ActivityScreen } from "@/screens/ActivityScreen";
import { SettingsScreen } from "@/screens/SettingsScreen";
import { PairScreen } from "@/screens/PairScreen";

export function App() {
  return (
    <Routes>
      <Route element={<AppShell />}>
        <Route index element={<Navigate to="/peers" replace />} />
        <Route path="peers" element={<PeersScreen />} />
        <Route path="send" element={<SendScreen />} />
        <Route path="inbox" element={<InboxScreen />} />
        <Route path="inbox/:itemId" element={<ItemDetailScreen />} />
        <Route path="sent" element={<SentScreen />} />
        <Route path="activity" element={<ActivityScreen />} />
        <Route path="settings" element={<SettingsScreen />} />
        <Route path="pair" element={<PairScreen />} />
        <Route path="*" element={<Navigate to="/peers" replace />} />
      </Route>
    </Routes>
  );
}
