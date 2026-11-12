// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { useParams } from "@tanstack/react-router";
import { TagsPage } from "./TagsPage";

export function TagDetailPage() {
    const { tagId } = useParams({ from: "/tags/$tagId" });
    return <TagsPage initialSelectedTagId={tagId} />;
}
