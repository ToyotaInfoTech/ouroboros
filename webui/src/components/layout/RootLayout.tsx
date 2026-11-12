// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { Outlet } from "@tanstack/react-router";
import { Sidebar } from "./Sidebar";

export function RootLayout() {
    return (
        <div className="flex h-screen bg-white">
            <Sidebar />
            <main className="flex-1 overflow-auto p-6">
                <Outlet />
            </main>
        </div>
    );
}
