// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { DashboardEvent } from "../api/types";
import { getAuthToken } from "../api/client";

/** Connect to the daemon WebSocket and invalidate queries on events. */
export function useWebSocket() {
    const queryClient = useQueryClient();
    const wsRef = useRef<WebSocket | null>(null);

    useEffect(() => {
        let cancelled = false;

        function doConnect() {
            if (cancelled) return;
            const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
            // The token travels as a subprotocol because browsers cannot set WebSocket headers.
            const token = getAuthToken();
            const protocols =
                token && /^[\w-]+$/.test(token) ? ["ouro.v1", `token.${token}`] : ["ouro.v1"];
            const ws = new WebSocket(`${protocol}//${window.location.host}/api/v1/ws`, protocols);

            ws.onmessage = (event) => {
                try {
                    const data: DashboardEvent = JSON.parse(event.data);
                    switch (data.type) {
                        case "EdgeStateChanged":
                        case "ShadowUpdated":
                        case "ShadowDeleted":
                            queryClient.invalidateQueries({ queryKey: ["shadows"] });
                            queryClient.invalidateQueries({ queryKey: ["edges"] });
                            break;
                        case "TagChanged":
                            queryClient.invalidateQueries({ queryKey: ["tags"] });
                            break;
                        case "TagAssignmentChanged":
                            queryClient.invalidateQueries({ queryKey: ["tags"] });
                            queryClient.invalidateQueries({ queryKey: ["edges"] });
                            queryClient.invalidateQueries({ queryKey: ["all-edge-tags"] });
                            break;
                    }
                } catch (err) {
                    console.warn("WebSocket: malformed message ignored", err);
                }
            };

            ws.onclose = () => {
                if (!cancelled) setTimeout(doConnect, 3000);
            };

            ws.onerror = () => {
                ws.close();
            };

            wsRef.current = ws;
        }

        doConnect();
        return () => {
            cancelled = true;
            wsRef.current?.close();
        };
    }, [queryClient]);
}
