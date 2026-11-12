// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import {
    createRouter,
    createRoute,
    createRootRoute,
} from "@tanstack/react-router";
import { lazy } from "react";
import { RootLayout } from "./components/layout/RootLayout";
import { DashboardPage } from "./pages/dashboard/DashboardPage";
import { TagsPage } from "./pages/tags/TagsPage";
import { TagDetailPage } from "./pages/tags/TagDetailPage";
import { EdgesPage } from "./pages/edges/EdgesPage";
import { EdgeDetailPage } from "./pages/edges/EdgeDetailPage";

const MetadataPage = lazy(() =>
    import("./pages/metadata/MetadataPage").then((m) => ({
        default: m.MetadataPage,
    })),
);
const MetadataDetailPage = lazy(() =>
    import("./pages/metadata/MetadataDetailPage").then((m) => ({
        default: m.MetadataDetailPage,
    })),
);

const rootRoute = createRootRoute({
    component: RootLayout,
});

const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: DashboardPage,
});

const tagsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/tags",
    component: TagsPage,
});

const tagDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/tags/$tagId",
    component: TagDetailPage,
});

const edgesRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/edges",
    component: EdgesPage,
});

const edgeDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/edges/$edgeId",
    component: EdgeDetailPage,
});

const metadataRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/metadata",
    component: MetadataPage,
});

const metadataDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/metadata/$vin/$startTime",
    component: MetadataDetailPage,
});

const routeTree = rootRoute.addChildren([
    indexRoute,
    tagsRoute,
    tagDetailRoute,
    edgesRoute,
    edgeDetailRoute,
    metadataRoute,
    metadataDetailRoute,
]);

export const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
    interface Register {
        router: typeof router;
    }
}