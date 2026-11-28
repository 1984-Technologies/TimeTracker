import { AuthGate } from "./auth";
import { DevicesPage } from "./pages/DevicesPage";
import { AdminsPage } from "./pages/AdminsPage";
import { Route, Routes } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { NotFoundPage } from "./pages/NotFoundPage";
import { UserPage } from "./pages/UserPage";
import { UsersPage } from "./pages/UsersPage";

export default function App() {
  return (
    <AuthGate><Routes>
      <Route element={<AppShell />}>
        <Route path="devices" element={<DevicesPage />} />
        <Route path="devices/register" element={<DevicesPage />} />
        <Route path="admins" element={<AdminsPage />} />
        <Route index element={<UsersPage />} />
        <Route path="users/:userId" element={<UserPage />} />
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes></AuthGate>
  );
}
