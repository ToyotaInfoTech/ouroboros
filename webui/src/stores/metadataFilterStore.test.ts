// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0


import { describe, it, expect, beforeEach } from "vitest";
import { useMetadataFilterStore } from "./metadataFilterStore";

describe("metadataFilterStore", () => {
    beforeEach(() => {
        // Reset store state before each test
        useMetadataFilterStore.setState({
            vinPattern: "",
            modelFilter: "",
            timeStart: "",
            timeEnd: "",
            geoBounds: null,
        });
    });

    it("has correct initial state", () => {
        const state = useMetadataFilterStore.getState();
        expect(state.vinPattern).toBe("");
        expect(state.modelFilter).toBe("");
        expect(state.timeStart).toBe("");
        expect(state.timeEnd).toBe("");
        expect(state.geoBounds).toBeNull();
    });

    it("setVinPattern updates vinPattern", () => {
        useMetadataFilterStore.getState().setVinPattern("VIN*");
        expect(useMetadataFilterStore.getState().vinPattern).toBe("VIN*");
    });

    it("setModelFilter updates modelFilter", () => {
        useMetadataFilterStore.getState().setModelFilter("sedan");
        expect(useMetadataFilterStore.getState().modelFilter).toBe("sedan");
    });

    it("setTimeStart updates timeStart", () => {
        useMetadataFilterStore.getState().setTimeStart("2026-01-01T00:00:00Z");
        expect(useMetadataFilterStore.getState().timeStart).toBe("2026-01-01T00:00:00Z");
    });

    it("setTimeEnd updates timeEnd", () => {
        useMetadataFilterStore.getState().setTimeEnd("2026-12-31T23:59:59Z");
        expect(useMetadataFilterStore.getState().timeEnd).toBe("2026-12-31T23:59:59Z");
    });

    it("setGeoBounds updates geoBounds", () => {
        const bounds = { min_lat: 35.0, max_lat: 36.0, min_lng: 136.0, max_lng: 137.0 };
        useMetadataFilterStore.getState().setGeoBounds(bounds);
        expect(useMetadataFilterStore.getState().geoBounds).toEqual(bounds);
    });

    it("setGeoBounds can clear to null", () => {
        const bounds = { min_lat: 35.0, max_lat: 36.0, min_lng: 136.0, max_lng: 137.0 };
        useMetadataFilterStore.getState().setGeoBounds(bounds);
        useMetadataFilterStore.getState().setGeoBounds(null);
        expect(useMetadataFilterStore.getState().geoBounds).toBeNull();
    });

    it("multiple setters are independent", () => {
        useMetadataFilterStore.getState().setVinPattern("VIN001");
        useMetadataFilterStore.getState().setModelFilter("SUV");
        const state = useMetadataFilterStore.getState();
        expect(state.vinPattern).toBe("VIN001");
        expect(state.modelFilter).toBe("SUV");
        expect(state.timeStart).toBe("");
    });
});
